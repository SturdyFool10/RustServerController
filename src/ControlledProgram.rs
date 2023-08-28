use serde::{Serialize, Deserialize};
use std::process::Stdio;
use tokio::{
    io::*,
    process::*,
    time::{Duration, *},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct ControlledProgramDescriptor {
    pub name: String,
    pub exePath: String,
    pub arguments: Vec<String>,
    pub working_dir: String,
}
impl ControlledProgramDescriptor {
    pub fn new(name: &str, exePath: &str, arguments: Vec<String>, working_dir: String) -> Self {
        Self {
            name: name.to_owned(),
            exePath: exePath.to_owned(),
            arguments,
            working_dir,
        }
    }
    pub fn into_instance(self) -> ControlledProgramInstance {
        ControlledProgramInstance::new(
            self.name.as_str(),
            self.exePath.as_str(),
            self.arguments,
            self.working_dir,
        )
    }
}

#[derive(Debug)]
pub struct ControlledProgramInstance {
    pub name: String,
    pub executablePath: String,
    pub commandLineArgs: Vec<String>,
    pub process: Child,
    pub working_dir: String,
    pub lastLogLines: usize,
}
impl ControlledProgramInstance {
    pub fn new(name: &str, exePath: &str, arguments: Vec<String>, working_dir: String) -> Self {
        let mut process = Command::new(exePath);
        let mut process = process.stdin(Stdio::piped());
        process = process.stdout(Stdio::piped());
        process = process.current_dir(working_dir.clone());
        for arg in arguments.iter() {
            process = process.arg(arg.replace("\\\\", "\\").replace('\"', ""));
        }
        let child = process
            .spawn()
            .expect("Could not spawn process for server.");
        Self {
            name: name.to_owned(),
            executablePath: exePath.to_owned(),
            commandLineArgs: arguments,
            process: child,
            working_dir,
            lastLogLines: 0,
        }
    }
    pub async fn readOutput(&mut self) -> Option<String> {
        let mut out = None;
        {
            let mut out2 = String::new();
            let mut line: usize = 0;
            let mut has_more = true;

            while has_more {
                let mut buf = [0u8; 1000];
                let take = self.process.stdout.as_mut();
                let read =
                    match timeout(Duration::from_millis(10), take.unwrap().read(&mut buf)).await {
                        Ok(val) => val.unwrap(),
                        Err(_) => 0,
                    };

                if read > 0 && line < read {
                    let new_str = String::from_utf8_lossy(&buf[0..read]);
                    out2.push_str(&new_str);
                    line = read;
                }

                if read < 100 {
                    has_more = false;
                }
            }

            out = Some(out2);
        };
        out
    }
}
