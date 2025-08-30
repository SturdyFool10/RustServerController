use super::ServerSpecialization;
use crate::controlled_program::ControlledProgramInstance;
use serde_json::{json, Value};

/// Terraria specialization struct.
/// Holds any state needed for Terraria-specific logic.
#[derive(Default)]
pub struct TerrariaSpecialization {
    // Add fields as needed for stateful logic
}

impl ServerSpecialization for TerrariaSpecialization {
    fn init(&mut self, instance: &mut ControlledProgramInstance) {
        // Initialize Terraria-specific state.
        // This is a stub for now.
        instance.specialized_server_info = Some(json!({
            "player_count": 0,
            "max_players": 0
        }));
    }

    fn parse_output(
        &mut self,
        line: String,
        instance: &mut ControlledProgramInstance,
    ) -> Option<String> {
        // Parse Terraria server output lines to update state.
        // This is a stub for now.
        let _ = (&line, instance); // silence unused warnings
        Some(line)
    }

    fn get_status(&self) -> Value {
        Value::Null
    }

    fn on_exit(
        &mut self,
        _instance: &mut crate::controlled_program::ControlledProgramInstance,
        _state: &crate::app_state::AppState,
        _exit_code: i32,
    ) {
        // Default: do nothing for Terraria
    }
}

/// Factory function for Terraria specialization.
pub fn factory() -> Box<dyn ServerSpecialization> {
    Box::new(TerrariaSpecialization::default())
}
