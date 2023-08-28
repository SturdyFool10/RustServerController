use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::Mutex;

use crate::ControlledProgram::ControlledProgramInstance;
use tokio::sync::broadcast;
#[derive(Clone)]
pub struct AppState {
    pub servers: Arc<Mutex<Vec<ControlledProgramInstance>>>,
    pub tx: broadcast::Sender<String>,
    running: Arc<AtomicBool>,
    socket_removal_queue: Arc<Mutex<Vec<usize>>>,
}
impl AppState {
    pub fn new(tx: broadcast::Sender<String>) -> Self {
        Self {
            servers: Arc::new(Mutex::new(vec![])),
            tx,
            running: Arc::new(AtomicBool::new(true)),
            socket_removal_queue: Arc::new(Mutex::new(vec![])),
        }
    }
    pub async fn remove_socket(&mut self, id: usize) {
        self.socket_removal_queue.lock().await.push(id);
    }
    pub fn stop(&mut self) {
        unsafe {
            let ptr = self.running.as_ptr();
            *ptr = false; //dereference the pointer and set it's value
        }
    }
    pub fn is_stopped(&self) -> bool {
        let mut ret = true;
        unsafe {
            let ptr = self.running.as_ptr();
            ret = *ptr; //clone the boolean stored in the pointer, do not take ownership
        }
        ret
    }
}
