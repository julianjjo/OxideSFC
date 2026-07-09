// Keyboard input handling - handled in frontend with keydown/keyup events
// This module is a placeholder for future keyboard input handling in Rust if needed
#![allow(dead_code)]

use std::collections::HashMap;

pub struct KeyboardState {
    pressed_keys: HashMap<String, bool>,
}

impl KeyboardState {
    pub fn new() -> Self {
        Self {
            pressed_keys: HashMap::new(),
        }
    }

    pub fn key_down(&mut self, key: String) {
        self.pressed_keys.insert(key, true);
    }

    pub fn key_up(&mut self, key: String) {
        self.pressed_keys.insert(key, false);
    }

    pub fn is_pressed(&self, key: &str) -> bool {
        self.pressed_keys.get(key).copied().unwrap_or(false)
    }
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self::new()
    }
}
