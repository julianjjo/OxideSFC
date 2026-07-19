/**
 * Canonical keyboard defaults for SNES input.
 *
 * Single source of truth for both the default key -> button mapping and the
 * button -> wire-format bitmask, so EmulatorView (live gameplay input) and
 * ControllerSettings (remapping UI) can never drift out of sync with each
 * other or with the bitmask EmulationController::set_controller_input
 * expects on the Rust side (oxidesfc-frontend/src-tauri/src/emulation/controller.rs).
 *
 * Key choice mirrors the classic Snes9x/ZSNES PC layout: three adjacent
 * QWERTY rows (Q/W, A/S, Z/X) map to the SNES's L/R, X/Y, A/B pairs, so the
 * physical key cluster echoes the controller's own diamond layout.
 */
import type { InputButton, KeyboardMapping } from './types';

export const DEFAULT_KEYBOARD_MAPPING: KeyboardMapping = {
  ArrowUp: 'up',
  ArrowDown: 'down',
  ArrowLeft: 'left',
  ArrowRight: 'right',
  KeyZ: 'a',
  KeyX: 'b',
  KeyA: 'x',
  KeyS: 'y',
  KeyQ: 'l',
  KeyW: 'r',
  Enter: 'start',
  ShiftRight: 'select',
};

export const SNES_BUTTON_BITMASK: Record<Exclude<InputButton, 'l_analog' | 'r_analog'>, number> = {
  up: 0x01,
  down: 0x02,
  left: 0x04,
  right: 0x08,
  a: 0x10,
  b: 0x20,
  start: 0x40,
  select: 0x80,
  l: 0x100,
  r: 0x200,
  x: 0x400,
  y: 0x800,
};
