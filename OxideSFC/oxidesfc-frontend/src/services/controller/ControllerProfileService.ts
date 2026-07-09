/**
 * Controller Profile Service
 * 
 * Manages multiple controller profiles with:
 * - Create, edit, delete profiles
 * - Profile types: Keyboard, Gamepad
 * - Per-profile button mappings
 * - Analog stick configuration
 * - Import/export as JSON
 * - Set default profile per game or globally
 */

import type {
  ControllerProfile,
  ControllerProfileType,
  ButtonMapping,
  AnalogStickConfig,
  ProfilePreset,
} from './types';
import {
  DEFAULT_KEYBOARD_MAPPING,
  DEFAULT_GAMEPAD_MAPPING,
  DEFAULT_ANALOG_CONFIG,
  KEYBOARD_PRESETS,
  GAMEPAD_PRESETS,
} from './types';

// ============================================================================
// Service State
// ============================================================================

interface ControllerProfileServiceState {
  profiles: Map<string, ControllerProfile>;
  activeProfileId: string | null;
  gameSpecificProfiles: Map<string, string>; // gameId -> profileId
}

// ============================================================================
// Controller Profile Service Implementation
// ============================================================================

class ControllerProfileServiceImpl {
  private state: ControllerProfileServiceState = {
    profiles: new Map(),
    activeProfileId: null,
    gameSpecificProfiles: new Map(),
  };

  constructor() {
    this.initializeDefaultProfiles();
  }

  /**
   * Initialize with default profiles
   */
  private initializeDefaultProfiles(): void {
    // Default keyboard profile
    const keyboardProfile: ControllerProfile = {
      id: 'profile-keyboard-default',
      name: 'Default Keyboard',
      type: 'keyboard',
      buttonMapping: DEFAULT_KEYBOARD_MAPPING,
      analogConfig: DEFAULT_ANALOG_CONFIG,
      isDefault: true,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
    this.state.profiles.set(keyboardProfile.id, keyboardProfile);
    this.state.activeProfileId = keyboardProfile.id;

    // Default gamepad profile
    const gamepadProfile: ControllerProfile = {
      id: 'profile-gamepad-default',
      name: 'Default Gamepad',
      type: 'gamepad',
      gamepadIndex: 0,
      buttonMapping: DEFAULT_GAMEPAD_MAPPING,
      analogConfig: DEFAULT_ANALOG_CONFIG,
      isDefault: false,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
    this.state.profiles.set(gamepadProfile.id, gamepadProfile);
  }

  // ============================================================================
  // Profile CRUD Operations
  // ============================================================================

  /**
   * Get all profiles
   */
  getProfiles(): ControllerProfile[] {
    return Array.from(this.state.profiles.values());
  }

  /**
   * Get profiles by type
   */
  getProfilesByType(type: ControllerProfileType): ControllerProfile[] {
    return this.getProfiles().filter(p => p.type === type);
  }

  /**
   * Get a specific profile by ID
   */
  getProfile(id: string): ControllerProfile | undefined {
    return this.state.profiles.get(id);
  }

  /**
   * Get the active profile
   */
  getActiveProfile(): ControllerProfile | null {
    if (!this.state.activeProfileId) return null;
    return this.state.profiles.get(this.state.activeProfileId) || null;
  }

  /**
   * Get the default profile
   */
  getDefaultProfile(): ControllerProfile | null {
    const profiles = this.getProfiles();
    return profiles.find(p => p.isDefault) || profiles[0] || null;
  }

  /**
   * Get profile for a specific game (game-specific or default)
   */
  getProfileForGame(gameId: string): ControllerProfile | null {
    const profileId = this.state.gameSpecificProfiles.get(gameId);
    if (profileId) {
      const profile = this.state.profiles.get(profileId);
      if (profile) return profile;
    }
    return this.getDefaultProfile();
  }

  /**
   * Create a new profile
   */
  createProfile(
    name: string,
    type: ControllerProfileType,
    mapping?: Partial<ButtonMapping>,
    gamepadIndex?: number
  ): ControllerProfile {
    const baseMapping = type === 'keyboard' ? DEFAULT_KEYBOARD_MAPPING : DEFAULT_GAMEPAD_MAPPING;
    
    const profile: ControllerProfile = {
      id: `profile-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      name,
      type,
      gamepadIndex,
      buttonMapping: { ...baseMapping, ...mapping },
      analogConfig: { ...DEFAULT_ANALOG_CONFIG },
      isDefault: false,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };

    this.state.profiles.set(profile.id, profile);
    return profile;
  }

  /**
   * Create a profile from a preset
   */
  createFromPreset(preset: ProfilePreset, name?: string): ControllerProfile {
    return this.createProfile(
      name || preset.name,
      preset.type,
      preset.mapping,
      preset.type === 'gamepad' ? 0 : undefined
    );
  }

  /**
   * Update a profile
   */
  updateProfile(id: string, updates: Partial<ControllerProfile>): boolean {
    const profile = this.state.profiles.get(id);
    if (!profile) return false;

    const updated: ControllerProfile = {
      ...profile,
      ...updates,
      id: profile.id, // Prevent ID change
      createdAt: profile.createdAt, // Prevent createdAt change
      updatedAt: new Date().toISOString(),
    };

    this.state.profiles.set(id, updated);
    return true;
  }

  /**
   * Delete a profile
   */
  deleteProfile(id: string): boolean {
    const profile = this.state.profiles.get(id);
    if (!profile) return false;

    // Don't allow deleting the last profile
    if (this.state.profiles.size <= 1) return false;

    // Don't allow deleting default if it's the only one of its type
    if (profile.isDefault) {
      const sameType = this.getProfilesByType(profile.type);
      if (sameType.length <= 1) return false;
    }

    // If deleting active profile, switch to another
    if (this.state.activeProfileId === id) {
      const others = this.getProfiles().filter(p => p.id !== id);
      this.state.activeProfileId = others[0]?.id || null;
    }

    // Clear any game-specific associations
    for (const [gameId, profileId] of this.state.gameSpecificProfiles) {
      if (profileId === id) {
        this.state.gameSpecificProfiles.delete(gameId);
      }
    }

    return this.state.profiles.delete(id);
  }

  /**
   * Set a profile as the default
   */
  setDefaultProfile(id: string): boolean {
    const profile = this.state.profiles.get(id);
    if (!profile) return false;

    // Unset other defaults of the same type
    this.state.profiles.forEach(p => {
      if (p.type === profile.type && p.isDefault) {
        this.state.profiles.set(p.id, { ...p, isDefault: false });
      }
    });

    return this.updateProfile(id, { isDefault: true });
  }

  /**
   * Set the active profile
   */
  setActiveProfile(id: string): boolean {
    const profile = this.state.profiles.get(id);
    if (!profile) return false;

    this.state.activeProfileId = id;
    return true;
  }

  /**
   * Set a profile as default for a specific game
   */
  setGameProfile(gameId: string, profileId: string): boolean {
    const profile = this.state.profiles.get(profileId);
    if (!profile) return false;

    this.state.gameSpecificProfiles.set(gameId, profileId);
    return true;
  }

  /**
   * Clear game-specific profile (revert to default)
   */
  clearGameProfile(gameId: string): boolean {
    return this.state.gameSpecificProfiles.delete(gameId);
  }

  // ============================================================================
  // Button Mapping Operations
  // ============================================================================

  /**
   * Update button mapping for a profile
   */
  updateButtonMapping(
    profileId: string,
    button: keyof ButtonMapping,
    inputId: string
  ): boolean {
    const profile = this.state.profiles.get(profileId);
    if (!profile) return false;

    return this.updateProfile(profileId, {
      buttonMapping: {
        ...profile.buttonMapping,
        [button]: inputId,
      },
    });
  }

  /**
   * Reset button mapping to defaults
   */
  resetButtonMapping(profileId: string): boolean {
    const profile = this.state.profiles.get(profileId);
    if (!profile) return false;

    const defaultMapping = profile.type === 'keyboard' 
      ? DEFAULT_KEYBOARD_MAPPING 
      : DEFAULT_GAMEPAD_MAPPING;

    return this.updateProfile(profileId, {
      buttonMapping: { ...defaultMapping },
    });
  }

  /**
   * Get available presets
   */
  getPresets(type: ControllerProfileType): ProfilePreset[] {
    return type === 'keyboard' ? KEYBOARD_PRESETS : GAMEPAD_PRESETS;
  }

  // ============================================================================
  // Analog Configuration Operations
  // ============================================================================

  /**
   * Update analog stick configuration
   */
  updateAnalogConfig(profileId: string, config: Partial<AnalogStickConfig>): boolean {
    const profile = this.state.profiles.get(profileId);
    if (!profile) return false;

    return this.updateProfile(profileId, {
      analogConfig: {
        ...profile.analogConfig,
        ...config,
      },
    });
  }

  /**
   * Reset analog configuration to defaults
   */
  resetAnalogConfig(profileId: string): boolean {
    return this.updateAnalogConfig(profileId, DEFAULT_ANALOG_CONFIG);
  }

  // ============================================================================
  // Import/Export
  // ============================================================================

  /**
   * Export profiles as JSON
   */
  exportProfiles(profileIds?: string[]): string {
    const profiles = profileIds 
      ? this.getProfiles().filter(p => profileIds.includes(p.id))
      : this.getProfiles();

    const exportData = {
      version: 1,
      exportedAt: new Date().toISOString(),
      profiles,
      gameSpecificProfiles: Object.fromEntries(this.state.gameSpecificProfiles),
    };

    return JSON.stringify(exportData, null, 2);
  }

  /**
   * Export a single profile as JSON
   */
  exportProfile(profileId: string): string | null {
    const profile = this.state.profiles.get(profileId);
    if (!profile) return null;

    const exportData = {
      version: 1,
      exportedAt: new Date().toISOString(),
      profile,
    };

    return JSON.stringify(exportData, null, 2);
  }

  /**
   * Import profiles from JSON
   */
  importProfiles(json: string, merge = true): { success: boolean; imported: number; errors: string[] } {
    const errors: string[] = [];
    let imported = 0;

    try {
      const data = JSON.parse(json);
      
      // Handle single profile import
      if (data.profile) {
        const profile = this.validateAndFixProfile(data.profile);
        if (profile) {
          this.state.profiles.set(profile.id, profile);
          imported = 1;
        } else {
          errors.push('Invalid profile data');
        }
        return { success: imported > 0, imported, errors };
      }

      // Handle bulk import
      const profiles = Array.isArray(data.profiles) ? data.profiles : [];
      
      if (!merge) {
        this.state.profiles.clear();
        this.state.gameSpecificProfiles.clear();
      }

      profiles.forEach((p: ControllerProfile) => {
        const profile = this.validateAndFixProfile(p);
        if (profile) {
          this.state.profiles.set(profile.id, profile);
          imported++;
        } else {
          errors.push(`Invalid profile: ${p.name || 'unknown'}`);
        }
      });

      // Import game-specific profiles if present
      if (data.gameSpecificProfiles && typeof data.gameSpecificProfiles === 'object') {
        Object.entries(data.gameSpecificProfiles).forEach(([gameId, profileId]) => {
          if (this.state.profiles.has(profileId as string)) {
            this.state.gameSpecificProfiles.set(gameId, profileId as string);
          }
        });
      }

      return { success: imported > 0, imported, errors };
    } catch (error) {
      errors.push(`Parse error: ${error instanceof Error ? error.message : 'Unknown error'}`);
      return { success: false, imported: 0, errors };
    }
  }

  /**
   * Validate and fix profile data
   */
  private validateAndFixProfile(data: Partial<ControllerProfile>): ControllerProfile | null {
    if (!data.name || !data.type) return null;

    return {
      id: data.id || `profile-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      name: data.name,
      type: data.type,
      gamepadIndex: data.gamepadIndex,
      buttonMapping: { ...DEFAULT_KEYBOARD_MAPPING, ...data.buttonMapping },
      analogConfig: { ...DEFAULT_ANALOG_CONFIG, ...data.analogConfig },
      isDefault: data.isDefault || false,
      gameId: data.gameId,
      createdAt: data.createdAt || new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
  }

  // ============================================================================
  // Utility Methods
  // ============================================================================

  /**
   * Get profile count
   */
  getProfileCount(): number {
    return this.state.profiles.size;
  }

  /**
   * Check if a profile name exists
   */
  nameExists(name: string, excludeId?: string): boolean {
    return this.getProfiles().some(p => 
      p.name.toLowerCase() === name.toLowerCase() && 
      p.id !== excludeId
    );
  }

  /**
   * Clone a profile
   */
  cloneProfile(id: string, newName: string): ControllerProfile | null {
    const profile = this.state.profiles.get(id);
    if (!profile) return null;

    return this.createProfile(
      newName,
      profile.type,
      profile.buttonMapping,
      profile.gamepadIndex
    );
  }
}

// ============================================================================
// Singleton Instance
// ============================================================================

export const ControllerProfileService = new ControllerProfileServiceImpl();

// ============================================================================
// Re-export Types
// ============================================================================

export type {
  ControllerProfile,
  ControllerProfileType,
  ButtonMapping,
  AnalogStickConfig,
  ProfilePreset,
};
