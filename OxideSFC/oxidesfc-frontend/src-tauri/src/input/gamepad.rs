use gilrs::{Gilrs, EventType, GamepadId};
use std::collections::HashMap;
use tracing::{debug, info, warn};

use super::InputEvent;

pub struct InputManager {
    gilrs: Option<Gilrs>,
    connected_controllers: HashMap<GamepadId, String>,
}

impl InputManager {
    pub fn new() -> Self {
        let gilrs = match Gilrs::new() {
            Ok(g) => {
                info!("Gamepad library initialized");
                Some(g)
            }
            Err(e) => {
                warn!("Failed to initialize gamepad library: {}", e);
                None
            }
        };

        Self {
            gilrs,
            connected_controllers: HashMap::new(),
        }
    }

    pub fn poll_events(&mut self) -> Vec<InputEvent> {
        let mut events = Vec::new();

        if let Some(ref mut gilrs) = self.gilrs {
            while let Some(event) = gilrs.next_event() {
                match event.event {
                    EventType::ButtonPressed(button, id) => {
                        debug!("Button pressed: {:?} on gamepad {:?}", button, id);
                        events.push(InputEvent {
                            event_type: "button_pressed".to_string(),
                            button: Some(format!("{:?}", button)),
                            value: None,
                            gamepad_id: Some(format!("{:?}", id)),
                        });
                    }
                    EventType::ButtonReleased(button, id) => {
                        debug!("Button released: {:?} on gamepad {:?}", button, id);
                        events.push(InputEvent {
                            event_type: "button_released".to_string(),
                            button: Some(format!("{:?}", button)),
                            value: None,
                            gamepad_id: Some(format!("{:?}", id)),
                        });
                    }
                    EventType::AxisChanged(axis, value, id) => {
                        events.push(InputEvent {
                            event_type: "axis_changed".to_string(),
                            button: Some(format!("{:?}", axis)),
                            value: Some(value),
                            gamepad_id: Some(format!("{:?}", id)),
                        });
                    }
                    EventType::Connected => {
                        let gamepad = gilrs.gamepad(event.id);
                        let name = gamepad.name().to_string();
                        info!("Gamepad connected: {} ({:?})", name, event.id);
                        self.connected_controllers.insert(event.id, name);
                        events.push(InputEvent {
                            event_type: "connected".to_string(),
                            button: None,
                            value: None,
                            gamepad_id: Some(format!("{:?}", event.id)),
                        });
                    }
                    EventType::Disconnected => {
                        info!("Gamepad disconnected: {:?}", event.id);
                        self.connected_controllers.remove(&event.id);
                        events.push(InputEvent {
                            event_type: "disconnected".to_string(),
                            button: None,
                            value: None,
                            gamepad_id: Some(format!("{:?}", event.id)),
                        });
                    }
                    _ => {}
                }
            }
        }

        events
    }

    pub fn connected_controllers(&self) -> Vec<(String, String)> {
        self.connected_controllers
            .iter()
            .map(|(id, name)| (format!("{:?}", id), name.clone()))
            .collect()
    }
}

impl Default for InputManager {
    fn default() -> Self {
        Self::new()
    }
}
