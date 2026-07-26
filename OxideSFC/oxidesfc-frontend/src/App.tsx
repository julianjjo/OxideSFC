import { useEffect, useState } from 'react';
import { Library } from './components/library/Library';
import { Settings } from './components/settings/Settings';
import { EmulatorView } from './components/emulator/EmulatorView';
import { WelcomeWizard } from './components/wizard/WelcomeWizard';
import { NavRail, type AppView } from './components/shell/NavRail';
import { useSettingsStore } from './stores/settingsStore';
import { useEmulationStore } from './stores/emulationStore';

function App() {
  const [currentView, setCurrentView] = useState<AppView>('library');
  const [showWizard, setShowWizard] = useState(false);
  const [wizardIsRerun, setWizardIsRerun] = useState(false);
  const { settings, hasLoaded, loadSettings, updateSettings } = useSettingsStore();
  const { isRunning, isPaused, currentGame } = useEmulationStore();

  // Load persisted settings once at startup so we know whether first-run
  // onboarding has already been completed. loadSettings() also pushes the
  // stored theme/accent onto <html> (see the store's syncAppearance), so there
  // is no separate appearance effect to keep in step here.
  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  // First-run check: show the wizard until the user finishes it once.
  //
  // Gated on `hasLoaded`, not on `isLoading`. `isLoading` is false before the
  // first load is even kicked off, and `has_completed_onboarding` defaults to
  // false, so the old `!isLoading && !completed` test fired on the very first
  // render for everyone -- and since this effect only ever *opens* the wizard,
  // the real persisted `true` arriving a moment later could not close it again.
  // Every returning user got the setup wizard on every launch.
  useEffect(() => {
    if (hasLoaded && !settings.general.has_completed_onboarding) {
      setShowWizard(true);
    }
  }, [hasLoaded, settings.general.has_completed_onboarding]);

  const handleWizardComplete = async () => {
    setShowWizard(false);
    setWizardIsRerun(false);
    try {
      await updateSettings({
        general: { ...settings.general, has_completed_onboarding: true },
      });
    } catch (error) {
      console.error('Failed to persist onboarding completion:', error);
    }
  };

  const handleWizardClose = () => {
    setShowWizard(false);
    setWizardIsRerun(false);
  };

  const handleRelaunchWizard = () => {
    setWizardIsRerun(true);
    setShowWizard(true);
  };

  // The emulator view is chromeless: the game gets the entire window and every
  // control lives in its own auto-hiding deck, so the rail steps aside during
  // play the same way the old header did.
  const inEmulator = currentView === 'emulator';

  return (
    <div className="flex h-screen overflow-hidden bg-void text-ink">
      {!inEmulator && (
        <NavRail
          view={currentView}
          onNavigate={setCurrentView}
          runningTitle={isRunning && currentGame ? currentGame.title : null}
          isPaused={isPaused}
        />
      )}

      <main className="min-w-0 flex-1 overflow-hidden">
        {currentView === 'library' && (
          <Library onPlayGame={() => setCurrentView('emulator')} />
        )}
        {currentView === 'settings' && (
          <Settings onRelaunchWizard={handleRelaunchWizard} />
        )}
        {currentView === 'emulator' && (
          <EmulatorView
            onExit={() => setCurrentView('library')}
            onOpenSettings={() => setCurrentView('settings')}
          />
        )}
      </main>

      <WelcomeWizard
        isOpen={showWizard}
        onComplete={handleWizardComplete}
        onClose={handleWizardClose}
        isRerun={wizardIsRerun}
      />
    </div>
  );
}

export default App;
