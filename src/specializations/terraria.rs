use super::ServerSpecialization;
use crate::controlled_program::ControlledProgramInstance;
use serde_json::{json, Value};

/// Specialization for Terraria servers.
///
/// Handles Terraria-specific logic such as parsing output and tracking player state.
/// Currently a stub for demonstration and extension.
#[derive(Default)]
pub struct TerrariaSpecialization {}

impl ServerSpecialization for TerrariaSpecialization {
    fn pre_init(
        &mut self,
        _env: &mut std::collections::HashMap<String, String>,
        _descriptor: &crate::controlled_program::ControlledProgramDescriptor,
    ) {
        // Default: do nothing for Terraria
    }

    /// Initialize the Terraria specialization for a server instance.
    ///
    /// Sets up the initial specialized_server_info state for player tracking.
    fn init(&mut self, instance: &mut ControlledProgramInstance) {
        // Initialize Terraria-specific state.
        // This is a stub for now.
        instance.specialized_server_info = Some(json!({
            "player_count": 0,
            "max_players": 0
        }));
    }

    /// Parses a single output line from the Terraria server process.
    ///
    /// Updates state as needed. Currently a stub that returns the line unchanged.
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

    /// Returns the current status for this specialization.

    ///

    /// For Terraria, this is always `Null` as status is stored in the instance's specialized_server_info.

    fn get_status(&self) -> Value {
        Value::Null
    }

    /// Returns true if the last processed log line resulted in a status update.
    /// Terraria stub: always false.
    fn has_status_update(&self) -> bool {
        false
    }

    /// Handles logic when the Terraria server process exits.
    ///
    /// Default implementation does nothing for Terraria.
    fn on_exit(
        &mut self,
        _instance: &mut crate::controlled_program::ControlledProgramInstance,
        _state: &crate::app_state::AppState,
        _exit_code: i32,
    ) {
        // Default: do nothing for Terraria
    }

    /// Sets the status update flag to false after an update has been sent.
    fn set_status_update_sent(&mut self) {
        // Terraria stub: nothing to do
    }
}

/// Factory function for Terraria specialization.
///
/// Returns a boxed instance of `TerrariaSpecialization`.
pub fn factory() -> Box<dyn ServerSpecialization> {
    Box::new(TerrariaSpecialization::default())
}
