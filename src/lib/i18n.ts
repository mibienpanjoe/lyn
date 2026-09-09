import type { LanguageSetting } from './ipc-types';

export interface Translations {
  settingsTitle: string;
  settingsSubtitle: string;
  saving: string;
  unsavedChanges: string;
  settingsSaved: string;
  loadingSettings: string;
  settingsLoadError: string;
  retry: string;

  quickCaptureTitle: string;
  quickCaptureSubtitle: string;
  globalShortcutLabel: string;
  changeShortcut: string;
  done: string;

  contextPriorityTitle: string;
  contextPrioritySubtitle: string;
  moveEarlier: (name: string) => string;
  moveLater: (name: string) => string;
  providers: {
    manual: string;
    vscode: string;
    cursor: string;
    browser: string;
    shell: string;
    foreground_window: string;
  };

  integrationsTitle: string;
  integrationsSubtitle: string;
  scanningIntegrations: string;
  refreshIntegrations: string;
  integrationDiscoveryFailed: string;
  badgeInstalled: string;
  badgeDetected: string;
  badgeReady: string;
  btnReinstall: string;
  btnEnableWatcher: string;
  btnRegisterHost: string;
  btnAddShell: string;
  btnInstallExtension: string;
  btnInstalling: string;
  integrationDescriptions: Record<string, string>;

  appearanceTitle: string;
  appearanceSubtitle: string;
  themeSystem: string;
  themeLight: string;
  themeDark: string;

  languageTitle: string;
  languageSubtitle: string;
  langEnglish: string;
  langFrench: string;

  speechTitle: string;
  speechSubtitle: string;
  modelApproxSize: string;
  modelLoading: string;
  modelDownloading: (percent: number) => string;
  modelInstalled: string;
  modelInstallFailed: string;
  modelNeedsRepair: string;
  modelNotInstalled: string;
  cancelDownload: string;
  removeModel: string;
  starting: string;
  retryInstall: string;
  installModel: string;
  downloadProgress: string;
  autoTranscription: string;
  autoTranscriptionDesc: string;

  navRecent: string;
  navAllCaptures: string;
  navSearch: string;
  navSettings: string;
  navProjects: string;
  navContexts: string;
  navAriaLabel: string;
  navToggleAriaLabel: string;
  branchLabel: string;
  allBranches: string;
  searchCapturesLabel: string;
  searchCapturesPlaceholder: string;
  contextFallback: string;
  copyText: string;
  copyTextCopied: string;
  copyTextFailed: string;
  deleteCapture: string;
  confirmDelete: string;
  cancelDelete: string;
  deleting: string;
  loadEarlier: string;
  kindText: string;
  kindImage: string;
  kindAudio: string;
  aboutTitle: string;
  aboutSubtitle: string;
  appVersionLabel: string;
  appVersionUnavailable: string;
  backToScope: (name: string) => string;
}

const en: Translations = {
  settingsTitle: 'Settings',
  settingsSubtitle: 'Local preferences for capture, context, and appearance.',
  saving: 'Saving…',
  unsavedChanges: 'Unsaved changes',
  settingsSaved: 'Settings saved',
  loadingSettings: 'Loading settings…',
  settingsLoadError: 'Settings could not be loaded.',
  retry: 'Retry',

  quickCaptureTitle: 'Quick capture',
  quickCaptureSubtitle:
    'The global shortcut used to open Lyn from another application.',
  globalShortcutLabel: 'Global shortcut',
  changeShortcut: 'Change shortcut',
  done: 'Done',

  contextPriorityTitle: 'Context tie-break order',
  contextPrioritySubtitle:
    'Used only when providers have equally strong invocation evidence.',
  moveEarlier: (name: string) => `Move ${name} earlier`,
  moveLater: (name: string) => `Move ${name} later`,
  providers: {
    manual: 'Manual selection',
    vscode: 'VS Code',
    cursor: 'Cursor',
    browser: 'Browser',
    shell: 'Terminal',
    foreground_window: 'Foreground window',
  },

  integrationsTitle: 'Integrations & Context Providers',
  integrationsSubtitle:
    'Connect your editors, browsers, and terminals with 1-click so Lyn automatically associates captures with your active workspace.',
  scanningIntegrations: 'Scanning local environment…',
  refreshIntegrations: 'Refresh integration statuses',
  integrationDiscoveryFailed: 'Integration discovery failed.',
  badgeInstalled: 'Installed',
  badgeDetected: 'Detected',
  badgeReady: 'Ready',
  btnReinstall: 'Reinstall',
  btnEnableWatcher: 'Enable Watcher',
  btnRegisterHost: 'Register Host',
  btnAddShell: 'Add to Shell',
  btnInstallExtension: 'Install Extension',
  btnInstalling: 'Installing…',
  integrationDescriptions: {
    cursor: 'Reports the focused Cursor workspace folder to Lyn on capture.',
    vscode: 'Reports the focused VS Code workspace folder to Lyn on capture.',
    browser:
      'Correlates active localhost development tabs with your repository context.',
    kitty:
      'Monitors exact focused terminal pane without inspecting commands or output.',
    shell:
      'Associates GNOME Terminal and external shells with current git repository.',
  },

  appearanceTitle: 'Appearance',
  appearanceSubtitle: 'Follow the system or choose a deterministic Lyn theme.',
  themeSystem: 'System',
  themeLight: 'Light',
  themeDark: 'Dark',

  languageTitle: 'Language',
  languageSubtitle: 'Choose your interface language. English is default.',
  langEnglish: 'English',
  langFrench: 'Français',

  speechTitle: 'Local speech',
  speechSubtitle:
    'Generate searchable captions for voice captures entirely on this device.',
  modelApproxSize: 'Approximately 150 MB',
  modelLoading: 'Loading model…',
  modelDownloading: (percent: number) => `Downloading ${percent}%`,
  modelInstalled: 'Installed',
  modelInstallFailed: 'Installation failed',
  modelNeedsRepair: 'Needs repair',
  modelNotInstalled: 'Model not installed',
  cancelDownload: 'Cancel download',
  removeModel: 'Remove model',
  starting: 'Starting…',
  retryInstall: 'Retry installation',
  installModel: 'Install model',
  downloadProgress: 'Download progress',
  autoTranscription: 'Automatic transcription',
  autoTranscriptionDesc: 'Generate a caption after saving each voice capture.',

  navRecent: 'Recent',
  navAllCaptures: 'All captures',
  navSearch: 'Search',
  navSettings: 'Settings',
  navProjects: 'Projects',
  navContexts: 'Contexts',
  navAriaLabel: 'Library navigation',
  navToggleAriaLabel: 'Toggle Library navigation',
  branchLabel: 'Branch',
  allBranches: 'All branches',
  searchCapturesLabel: 'Search captures',
  searchCapturesPlaceholder: 'Search text and captions',
  contextFallback: 'Context',
  copyText: 'Copy text',
  copyTextCopied: 'Copied',
  copyTextFailed: 'Copy failed',
  deleteCapture: 'Delete capture',
  confirmDelete: 'Delete',
  cancelDelete: 'Cancel',
  deleting: 'Deleting…',
  loadEarlier: 'Load earlier captures',
  kindText: 'Text',
  kindImage: 'Screenshot',
  kindAudio: 'Voice note',
  aboutTitle: 'About',
  aboutSubtitle: 'The version of Lyn running on this computer.',
  appVersionLabel: 'Version',
  appVersionUnavailable: 'Unavailable',
  backToScope: (name: string) => `Back to ${name}`,
};

const fr: Translations = {
  settingsTitle: 'Paramètres',
  settingsSubtitle:
    'Préférences locales pour la capture, le contexte et l’apparence.',
  saving: 'Enregistrement…',
  unsavedChanges: 'Modifications non enregistrées',
  settingsSaved: 'Paramètres enregistrés',
  loadingSettings: 'Chargement des paramètres…',
  settingsLoadError: 'Impossible de charger les paramètres.',
  retry: 'Réessayer',

  quickCaptureTitle: 'Capture rapide',
  quickCaptureSubtitle:
    'Le raccourci global utilisé pour ouvrir Lyn depuis une autre application.',
  globalShortcutLabel: 'Raccourci global',
  changeShortcut: 'Modifier le raccourci',
  done: 'Terminé',

  contextPriorityTitle: 'Ordre de priorité du contexte',
  contextPrioritySubtitle:
    'Utilisé uniquement lorsque les fournisseurs ont des preuves d’invocation d’égale importance.',
  moveEarlier: (name: string) => `Avancer ${name}`,
  moveLater: (name: string) => `Reculer ${name}`,
  providers: {
    manual: 'Sélection manuelle',
    vscode: 'VS Code',
    cursor: 'Cursor',
    browser: 'Navigateur',
    shell: 'Terminal',
    foreground_window: 'Fenêtre active',
  },

  integrationsTitle: 'Intégrations & Fournisseurs de contexte',
  integrationsSubtitle:
    'Connectez vos éditeurs, navigateurs et terminaux en 1 clic pour que Lyn associe automatiquement vos captures à votre espace de travail.',
  scanningIntegrations: 'Analyse de l’environnement local…',
  refreshIntegrations: 'Actualiser l’état des intégrations',
  integrationDiscoveryFailed: 'Échec de la détection des intégrations.',
  badgeInstalled: 'Installé',
  badgeDetected: 'Détecté',
  badgeReady: 'Prêt',
  btnReinstall: 'Réinstaller',
  btnEnableWatcher: 'Activer le watcher',
  btnRegisterHost: 'Enregistrer l’hôte',
  btnAddShell: 'Ajouter au shell',
  btnInstallExtension: 'Installer l’extension',
  btnInstalling: 'Installation…',
  integrationDescriptions: {
    cursor:
      'Transmet le dossier de travail Cursor actif à Lyn lors de la capture.',
    vscode:
      'Transmet le dossier de travail VS Code actif à Lyn lors de la capture.',
    browser:
      'Associe les onglets de développement localhost actifs au contexte de votre dépôt.',
    kitty:
      'Surveille le volet de terminal actif sans inspecter les commandes ni les sorties.',
    shell: 'Associe GNOME Terminal et les shells externes au dépôt Git actuel.',
  },

  appearanceTitle: 'Apparence',
  appearanceSubtitle:
    'Suivre le système ou choisir un thème déterministe pour Lyn.',
  themeSystem: 'Système',
  themeLight: 'Clair',
  themeDark: 'Sombre',

  languageTitle: 'Langue',
  languageSubtitle:
    'Choisissez la langue de l’interface. L’anglais est la langue par défaut.',
  langEnglish: 'English',
  langFrench: 'Français',

  speechTitle: 'Reconnaissance vocale locale',
  speechSubtitle:
    'Générez des sous-titres indexables pour vos captures vocales directement sur cet appareil.',
  modelApproxSize: 'Environ 150 Mo',
  modelLoading: 'Chargement du modèle…',
  modelDownloading: (percent: number) => `Téléchargement ${percent}%`,
  modelInstalled: 'Installé',
  modelInstallFailed: 'Échec de l’installation',
  modelNeedsRepair: 'Réparation requise',
  modelNotInstalled: 'Modèle non installé',
  cancelDownload: 'Annuler le téléchargement',
  removeModel: 'Supprimer le modèle',
  starting: 'Démarrage…',
  retryInstall: 'Réessayer l’installation',
  installModel: 'Installer le modèle',
  downloadProgress: 'Progression du téléchargement',
  autoTranscription: 'Transcription automatique',
  autoTranscriptionDesc:
    'Générer une légende après avoir enregistré chaque capture vocale.',

  navRecent: 'Récents',
  navAllCaptures: 'Toutes les captures',
  navSearch: 'Recherche',
  navSettings: 'Paramètres',
  navProjects: 'Projets',
  navContexts: 'Contextes',
  navAriaLabel: 'Navigation de la bibliothèque',
  navToggleAriaLabel: 'Afficher/masquer la navigation',
  branchLabel: 'Branche',
  allBranches: 'Toutes les branches',
  searchCapturesLabel: 'Rechercher dans les captures',
  searchCapturesPlaceholder: 'Rechercher texte et légendes',
  contextFallback: 'Contexte',
  copyText: 'Copier le texte',
  copyTextCopied: 'Copié',
  copyTextFailed: 'Échec de la copie',
  deleteCapture: 'Supprimer la capture',
  confirmDelete: 'Supprimer',
  cancelDelete: 'Annuler',
  deleting: 'Suppression…',
  loadEarlier: 'Charger les captures précédentes',
  kindText: 'Texte',
  kindImage: 'Capture d’écran',
  kindAudio: 'Note vocale',
  aboutTitle: 'À propos',
  aboutSubtitle: 'La version de Lyn installée sur cet ordinateur.',
  appVersionLabel: 'Version',
  appVersionUnavailable: 'Indisponible',
  backToScope: (name: string) => `Retour vers ${name}`,
};

export function getTranslations(
  lang: LanguageSetting = 'english',
): Translations {
  return lang === 'french' ? fr : en;
}
