use serde_json::Value;
use std::{path::PathBuf, sync::Arc};
use wasmtime::{Engine, Instance, Memory, Module, Store, TypedFunc};

const MAX_WASM_IO_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct WasmPluginRuntime {
    engine: Engine,
    module: Arc<Module>,
    module_path: PathBuf,
}

impl WasmPluginRuntime {
    pub fn load(module_path: PathBuf) -> Option<Self> {
        let engine = Engine::default();
        let module = match Module::from_file(&engine, &module_path) {
            Ok(module) => module,
            Err(error) => {
                tracing::warn!("Failed to load WASM plugin {:?}: {}", module_path, error);
                return None;
            }
        };
        Some(Self {
            engine,
            module: Arc::new(module),
            module_path,
        })
    }

    pub fn call_json_hook(&self, hook: &str, input: &Value) -> Option<Value> {
        let input = serde_json::to_string(input).ok()?;
        let output = self.call_string_hook(hook, &input)?;
        serde_json::from_str(&output).ok()
    }

    fn call_string_hook(&self, hook: &str, input: &str) -> Option<String> {
        let mut store = Store::new(&self.engine, ());
        store.set_fuel(5_000_000).ok();
        let instance = Instance::new(&mut store, &self.module, &[])
            .map_err(log_wasm_error)
            .ok()?;
        let memory = instance.get_memory(&mut store, "memory").or_else(|| {
            tracing::warn!("WASM plugin {:?} does not export memory", self.module_path);
            None
        })?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "rsc_alloc")
            .map_err(log_wasm_error)
            .ok()?;
        let dealloc = instance
            .get_typed_func::<(i32, i32), ()>(&mut store, "rsc_dealloc")
            .ok();
        let hook_func = instance
            .get_typed_func::<(i32, i32), i64>(&mut store, hook)
            .ok()?;
        let input_ptr = write_guest_string(&mut store, &memory, &alloc, input)?;
        let packed = hook_func
            .call(&mut store, (input_ptr, i32::try_from(input.len()).ok()?))
            .map_err(log_wasm_error)
            .ok()?;
        if let Some(dealloc) = &dealloc {
            let _ = dealloc.call(
                &mut store,
                (input_ptr, i32::try_from(input.len()).unwrap_or_default()),
            );
        }
        let (output_ptr, output_len) = unpack_ptr_len(packed)?;
        let output = read_guest_string(&mut store, &memory, output_ptr, output_len)?;
        if let Some(dealloc) = &dealloc {
            let _ = dealloc.call(&mut store, (output_ptr, output_len));
        }
        Some(output)
    }
}

fn write_guest_string(
    store: &mut Store<()>,
    memory: &Memory,
    alloc: &TypedFunc<i32, i32>,
    input: &str,
) -> Option<i32> {
    if input.len() > MAX_WASM_IO_BYTES {
        return None;
    }
    let len = i32::try_from(input.len()).ok()?;
    let ptr = alloc.call(&mut *store, len).map_err(log_wasm_error).ok()?;
    if ptr < 0 {
        return None;
    }
    memory
        .write(&mut *store, usize::try_from(ptr).ok()?, input.as_bytes())
        .map_err(log_wasm_error)
        .ok()?;
    Some(ptr)
}

fn read_guest_string(store: &mut Store<()>, memory: &Memory, ptr: i32, len: i32) -> Option<String> {
    if ptr < 0 || len < 0 || usize::try_from(len).ok()? > MAX_WASM_IO_BYTES {
        return None;
    }
    let mut bytes = vec![0; usize::try_from(len).ok()?];
    memory
        .read(&mut *store, usize::try_from(ptr).ok()?, &mut bytes)
        .map_err(log_wasm_error)
        .ok()?;
    String::from_utf8(bytes).ok()
}

fn unpack_ptr_len(value: i64) -> Option<(i32, i32)> {
    let ptr = i32::try_from((value >> 32) & 0xffff_ffff).ok()?;
    let len = i32::try_from(value & 0xffff_ffff).ok()?;
    Some((ptr, len))
}

fn log_wasm_error(error: impl std::fmt::Display) {
    tracing::warn!("WASM plugin hook failed: {}", error);
}
