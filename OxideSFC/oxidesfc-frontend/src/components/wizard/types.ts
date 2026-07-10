/**
 * Wizard Types
 * 
 * Type definitions for the first-time setup wizard.
 */

import type { ControllerProfileType, ButtonMapping, AnalogStickConfig } from '../../services/controller';

// ============================================================================
// Wizard Step
// ============================================================================

export type WizardStep = 
  | 'welcome'
  | 'language'
  | 'rom-folder'
  | 'controller-type'
  | 'controller-profile'
  | 'audio-video'
  | 'metadata'
  | 'complete';

// ============================================================================
// Step State
// ============================================================================

export interface WizardState {
  currentStep: WizardStep;
  completedSteps: Set<WizardStep>;
  isComplete: boolean;
  canGoBack: boolean;
  canSkip: boolean;
}

// ============================================================================
// Language Options
// ============================================================================

export interface LanguageOption {
  code: string;
  name: string;
  nativeName: string;
}

export const LANGUAGE_OPTIONS: LanguageOption[] = [
  { code: 'en', name: 'English', nativeName: 'English' },
  { code: 'es', name: 'Spanish', nativeName: 'Español' },
  { code: 'fr', name: 'French', nativeName: 'Français' },
  { code: 'de', name: 'German', nativeName: 'Deutsch' },
  { code: 'it', name: 'Italian', nativeName: 'Italiano' },
  { code: 'pt', name: 'Portuguese', nativeName: 'Português' },
  { code: 'ja', name: 'Japanese', nativeName: '日本語' },
  { code: 'ko', name: 'Korean', nativeName: '한국어' },
  { code: 'zh', name: 'Chinese', nativeName: '中文' },
  { code: 'ru', name: 'Russian', nativeName: 'Русский' },
];

// ============================================================================
// Controller Setup Data
// ============================================================================

export interface ControllerSetupData {
  type: ControllerProfileType;
  name: string;
  buttonMapping: ButtonMapping;
  analogConfig: AnalogStickConfig;
  gamepadIndex?: number;
}

// ============================================================================
// Wizard Form Data
// ============================================================================

export interface WizardFormData {
  // Step 1: Language
  language: string;
  
  // Step 2: ROM Folder
  romFolder: string;
  scanSubfolders: boolean;
  
  // Step 3: Controller Type
  controllerType: ControllerProfileType;
  
  // Step 4: Controller Profile
  controllerProfile: ControllerSetupData;
  
  // Step 5: Audio/Video
  videoSettings: {
    vsync: boolean;
    renderer: string;
    shader: string;
    scaleMode: string;
  };
  audioSettings: {
    enabled: boolean;
    volume: number;
  };
  
  // Step 6: Metadata
  metadataSettings: {
    enabled: boolean;
    preferredSource: string;
  };
}

// ============================================================================
// Default Form Data
// ============================================================================

export const DEFAULT_WIZARD_FORM_DATA: WizardFormData = {
  language: 'en',
  romFolder: '',
  scanSubfolders: true,
  controllerType: 'keyboard',
  controllerProfile: {
    type: 'keyboard',
    name: 'Default',
    buttonMapping: {
      up: 'ArrowUp',
      down: 'ArrowDown',
      left: 'ArrowLeft',
      right: 'ArrowRight',
      a: 'KeyZ',
      b: 'KeyX',
      x: 'KeyA',
      y: 'KeyS',
      l: 'KeyQ',
      r: 'KeyW',
      start: 'Enter',
      select: 'ShiftRight',
    },
    analogConfig: {
      invertX: false,
      invertY: false,
      deadzone: 0.15,
      sensitivity: 1.0,
    },
  },
  videoSettings: {
    vsync: true,
    renderer: 'webgl',
    shader: 'none',
    scaleMode: 'nearest',
  },
  audioSettings: {
    enabled: true,
    volume: 0.8,
  },
  metadataSettings: {
    enabled: true,
    preferredSource: 'screenscraper',
  },
};

// ============================================================================
// Step Configuration
// ============================================================================

export interface StepConfig {
  id: WizardStep;
  title: string;
  description: string;
  canSkip: boolean;
  isRequired: boolean;
}

export const WIZARD_STEPS: StepConfig[] = [
  {
    id: 'welcome',
    title: 'Welcome',
    description: 'Welcome to OxideSFC! Let\'s get you set up.',
    canSkip: false,
    isRequired: true,
  },
  {
    id: 'language',
    title: 'Language',
    description: 'Select your preferred language',
    canSkip: false,
    isRequired: true,
  },
  {
    id: 'rom-folder',
    title: 'ROM Folder',
    description: 'Choose where your ROM files are located',
    canSkip: true,
    isRequired: false,
  },
  {
    id: 'controller-type',
    title: 'Controller Type',
    description: 'Are you using a keyboard or gamepad?',
    canSkip: false,
    isRequired: true,
  },
  {
    id: 'controller-profile',
    title: 'Controller Profile',
    description: 'Configure your controller buttons',
    canSkip: false,
    isRequired: true,
  },
  {
    id: 'audio-video',
    title: 'Audio & Video',
    description: 'Configure basic video and audio settings',
    canSkip: true,
    isRequired: false,
  },
  {
    id: 'metadata',
    title: 'Metadata',
    description: 'Fetch game information from online sources',
    canSkip: true,
    isRequired: false,
  },
  {
    id: 'complete',
    title: 'Complete!',
    description: 'You\'re all set!',
    canSkip: false,
    isRequired: true,
  },
];
