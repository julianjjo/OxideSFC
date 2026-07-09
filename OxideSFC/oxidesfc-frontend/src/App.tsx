import { useEffect, useState } from 'react';
import { Library } from './components/library/Library';
import { Settings } from './components/settings/Settings';
import { EmulatorView } from './components/emulator/EmulatorView';
import { WelcomeWizard } from './components/wizard/WelcomeWizard';
import { useSettingsStore } from './stores/settingsStore';
import { useEmulationStore } from './stores/emulationStore';

type View = 'library' | 'settings' | 'emulator';

function App() {
  const [currentView, setCurrentView] = useState<View>('library');
  const [showWizard, setShowWizard] = useState(false);
  const [wizardIsRerun, setWizardIsRerun] = useState(false);
  const { settings, isLoading, loadSettings, updateSettings } = useSettingsStore();
  const { isRunning } = useEmulationStore();

  // Load persisted settings once at startup so we know whether first-run
  // onboarding has already been completed.
  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  // First-run check: show the wizard until the user finishes it once. Wait
  // for the real persisted value to load before deciding, so returning
  // users don't see the wizard flash before settings arrive from disk.
  useEffect(() => {
    if (!isLoading && !settings.general.has_completed_onboarding) {
      setShowWizard(true);
    }
  }, [isLoading, settings.general.has_completed_onboarding]);

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

  // Apply theme
  const theme = settings.general.theme || 'dark';

  return (
    <div className={`h-screen flex flex-col ${theme === 'light' ? 'bg-gray-100' : 'bg-slate-900'}`}>
      {/* Header / Navigation */}
      <header className={`flex items-center justify-between px-4 py-3 ${theme === 'light' ? 'bg-white border-b border-gray-200' : 'bg-slate-800 border-b border-slate-700'}`}>
        <div className="flex items-center gap-6">
          <h1 className="text-xl font-bold text-primary-500">OxideSFC</h1>
          <nav className="flex gap-4">
            <button
              onClick={() => setCurrentView('library')}
              className={`px-3 py-1.5 rounded-md transition-colors ${
                currentView === 'library'
                  ? 'bg-primary-600 text-white'
                  : theme === 'light'
                  ? 'text-gray-700 hover:bg-gray-100'
                  : 'text-slate-300 hover:bg-slate-700'
              }`}
            >
              Library
            </button>
            <button
              onClick={() => setCurrentView('settings')}
              className={`px-3 py-1.5 rounded-md transition-colors ${
                currentView === 'settings'
                  ? 'bg-primary-600 text-white'
                  : theme === 'light'
                  ? 'text-gray-700 hover:bg-gray-100'
                  : 'text-slate-300 hover:bg-slate-700'
              }`}
            >
              Settings
            </button>
          </nav>
        </div>

        {isRunning && (
          <button
            onClick={() => setCurrentView('emulator')}
            className="px-4 py-1.5 bg-red-600 hover:bg-red-700 rounded-md transition-colors"
          >
            Back to Game
          </button>
        )}
      </header>

      {/* Main Content */}
      <main className="flex-1 overflow-hidden">
        {currentView === 'library' && <Library onPlayGame={() => setCurrentView('emulator')} />}
        {currentView === 'settings' && <Settings onRelaunchWizard={handleRelaunchWizard} />}
        {currentView === 'emulator' && <EmulatorView onExit={() => setCurrentView('library')} />}
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
