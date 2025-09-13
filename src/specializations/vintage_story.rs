use std::path::Path;
use tracing::{error, warn};

use crate::{controlled_program::ControlledProgramInstance, specializations::ServerSpecialization};
use futures::future::join_all;
use std::fs::read_dir;
use tracing::info;

pub struct VintageStoryServerSpecialization;

impl ServerSpecialization for VintageStoryServerSpecialization {
    fn init(&mut self, instance: &mut ControlledProgramInstance) {
        let exe_path = Path::new(instance.executable_path.as_str());
        let parent_dir = match exe_path.parent() {
            Some(dir) => dir.to_path_buf(),
            None => {
                error!(
                    "Failed to get parent directory of executable: {}",
                    instance.executable_path
                );
                return;
            }
        };
        let working_dir = Path::new(instance.working_dir.as_str());
        //vintage story folders contain some dlls, which are required for the game to run, and are located in the same directory as the executable.
        // walk the parent dir for dlls, create a list of parent relative paths to dlls so we can copy them in paralell using tokio tasks
        let mut dlls: Vec<String> = Vec::new();
        let mut dirs = Vec::new();
        //use iterators to recursively search for dlls, we also need their relative path so we can recreate the folder structure in the server instance folder
        dirs.push(parent_dir.clone());
        while let Some(dir) = dirs.pop() {
            let entries = match read_dir(&dir) {
                Ok(e) => e,
                Err(e) => {
                    error!("Failed to read directory {:?}: {}", dir, e);
                    continue;
                }
            };
            for entry_result in entries {
                let entry = match entry_result {
                    Ok(e) => e,
                    Err(e) => {
                        warn!("Failed to read directory entry in {:?}: {}", dir, e);
                        continue;
                    }
                };
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "dll" {
                            match path.strip_prefix(&parent_dir) {
                                Ok(rel_path) => match rel_path.to_str() {
                                    Some(s) => dlls.push(s.to_string()),
                                    None => {
                                        warn!("Failed to convert path {:?} to string", rel_path)
                                    }
                                },
                                Err(e) => {
                                    warn!("Failed to get relative path for {:?}: {}", path, e)
                                }
                            }
                        }
                    }
                }
            }
        }

        // Prepare async copy tasks for all DLLs, ensuring directories exist and logging results
        let copy_futures = dlls.iter().map(|dll_rel| {
            let source_path = parent_dir.join(dll_rel);
            let dest_path = working_dir.join(dll_rel);
            let dest_dir = dest_path.parent().map(|p| p.to_path_buf());
            async move {
                if let Some(dir) = dest_dir {
                    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
                        error!("Failed to create directory {:?}: {}", dir, e);
                        return;
                    }
                }
                match tokio::fs::copy(&source_path, &dest_path).await {
                    Ok(_) => info!("Copied {:?} to {:?}", source_path, dest_path),
                    Err(e) => error!("Failed to copy {:?} to {:?}: {}", source_path, dest_path, e),
                }
            }
        });
        // Await all copy tasks to ensure completion before proceeding
        tokio::runtime::Handle::current().block_on(join_all(copy_futures));
    }
    fn parse_output(
        &mut self,
        line: String,
        instance: &mut ControlledProgramInstance,
    ) -> Option<String> {
        Some(line)
    }
    fn get_status(&self) -> serde_json::Value {
        serde_json::json!({})
    }
}
