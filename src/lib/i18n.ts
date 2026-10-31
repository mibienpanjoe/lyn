import type {
  GnomeActivationFilesStatus,
  GnomeActivationPhase,
  GnomeManagementAction,
  GnomeConnectionStatus,
  LanguageSetting,
} from './ipc-types';

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
  btnInstallNative: string;
  gnomeConnectionStatus: Record<GnomeConnectionStatus, string>;
  gnomeFilesStatus: Record<GnomeActivationFilesStatus, string>;
  gnomeActivationPhase: Record<GnomeActivationPhase, string>;
  gnomeInstallUnavailable: string;
  gnomeManagementLabels: Record<GnomeManagementAction, string>;
  gnomeManagementReceipts: Record<GnomeManagementAction, string>;
  gnomeManaging: string;
  gnomeManagementFailed: string;
  gnomeManagementStale: string;
  gnomeManagementRefreshFailed: string;
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
  voiceCaption: string;
  voiceCaptionLimit: string;
  voiceCaptionSaving: string;
  voiceCaptionSave: string;
  voiceCaptionCancel: string;
  voiceCaptionGenerated: string;
  voiceCaptionEdit: string;
  voiceTranscribing: string;
  voiceTranscriptionFailed: string;
  voiceTranscriptionRetry: string;
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
  deleteProject: string;
  confirmProjectDelete: (name: string) => string;
  projectNotEmpty: string;
  projectDeleteFailed: string;
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
    'Opens Lyn from any app. Editor, Cursor, Kitty, and the browser companion use Alt+L by default.',
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
  btnInstallNative: 'Install',
  gnomeConnectionStatus: {
    connected: 'Connected',
    disconnected: 'Not connected',
    busy: 'Connection busy',
    unavailable: 'Connection unavailable',
  },
  gnomeFilesStatus: {
    absent: 'Not installed.',
    owned: 'Activation files verified.',
    changed: 'Activation files changed.',
    pending: 'Installation needs attention.',
    unavailable: 'Installation status unavailable.',
  },
  gnomeInstallUnavailable: 'Installation is not available in this build.',
  gnomeManagementLabels: {
    update: 'Update',
    rollback: 'Restore previous version',
    remove: 'Remove configuration',
    uninstall: 'Remove integration',
    recover_install: 'Finish installation',
    finish_update: 'Finish update',
    restore_update: 'Restore configuration',
    recover_removal: 'Finish removal',
    synchronize: 'Apply configuration',
    confirm_removal: 'Finish removal',
  },
  gnomeManagementReceipts: {
    update: 'Configuration updated. Open terminals remain unchanged.',
    rollback:
      'Previous configuration restored. Open terminals remain unchanged.',
    remove:
      'Configuration removed. Finish removal before starting GNOME Terminal again.',
    uninstall: 'Integration removed. Open terminals remain unchanged.',
    recover_install:
      'Installation prepared. Apply the configuration before the next GNOME Terminal start.',
    finish_update: 'Update completed. Open terminals remain unchanged.',
    restore_update:
      'Previous configuration restored. Open terminals remain unchanged.',
    recover_removal:
      'Configuration removed. Finish removal before starting GNOME Terminal again.',
    synchronize:
      'Configuration applied to future GNOME Terminal starts. Open terminals remain unchanged.',
    confirm_removal: 'Integration removed. Open terminals remain unchanged.',
  },
  gnomeManaging: 'Working…',
  gnomeManagementFailed:
    'The operation could not be completed. Check the refreshed status before acting again.',
  gnomeManagementStale:
    'The configuration changed. Check the refreshed status before acting again.',
  gnomeManagementRefreshFailed:
    'The status could not be refreshed. Refresh it before acting again.',
  gnomeActivationPhase: {
    absent: 'Not installed.',
    configured: 'Lyn configuration verified.',
    installing: 'Installation needs to be completed.',
    finishing_update: 'Update needs to be completed.',
    restoring_update: 'Previous configuration needs to be restored.',
    removing: 'Removal needs to be completed.',
    awaiting_removal_confirmation: 'Removal needs to be confirmed.',
    changed: 'Configuration changed; verification is required.',
    unavailable: 'Installation status unavailable.',
  },
  integrationDescriptions: {
    gnome: 'Associates captures with the active local GNOME terminal project.',
    cursor: 'Reports the focused Cursor workspace folder to Lyn on capture.',
    vscode: 'Reports the focused VS Code workspace folder to Lyn on capture.',
    browser:
      'Captures from the focused tab; localhost maps to a project only when one local process directory is proven.',
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
  modelApproxSize: 'Approximately 942 MB',
  voiceCaption: 'Voice caption',
  voiceCaptionLimit:
    'Up to 500 characters. Your correction takes precedence over transcription.',
  voiceCaptionSaving: 'Saving…',
  voiceCaptionSave: 'Save caption',
  voiceCaptionCancel: 'Cancel',
  voiceCaptionGenerated: 'Generated caption',
  voiceCaptionEdit: 'Edit caption',
  voiceTranscribing: 'Transcribing on this device…',
  voiceTranscriptionFailed: 'Transcription failed. Your recording is saved.',
  voiceTranscriptionRetry: 'Retry transcription',
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
  deleteProject: 'Delete project',
  confirmProjectDelete: (name) =>
    `Remove “${name}” from the Library? Its folder stays on disk.`,
  projectNotEmpty: 'This project still has captures. Delete them first.',
  projectDeleteFailed: 'The project could not be deleted. Try again.',
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
    'Ouvre Lyn depuis n’importe quelle application. L’éditeur, Cursor, Kitty et le compagnon navigateur utilisent Alt+L par défaut.',
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
  btnInstallNative: 'Installer',
  gnomeConnectionStatus: {
    connected: 'Connecté',
    disconnected: 'Non connecté',
    busy: 'Connexion occupée',
    unavailable: 'Connexion indisponible',
  },
  gnomeFilesStatus: {
    absent: 'Non installé.',
    owned: 'Fichiers d’activation vérifiés.',
    changed: 'Fichiers d’activation modifiés.',
    pending: 'L’installation nécessite une intervention.',
    unavailable: 'État de l’installation indisponible.',
  },
  gnomeInstallUnavailable:
    'L’installation n’est pas disponible dans cette version.',
  gnomeManagementLabels: {
    update: 'Mettre à jour',
    rollback: 'Revenir à la version précédente',
    remove: 'Retirer la configuration',
    uninstall: 'Retirer l’intégration',
    recover_install: 'Terminer l’installation',
    finish_update: 'Terminer la mise à jour',
    restore_update: 'Restaurer la configuration',
    recover_removal: 'Terminer le retrait',
    synchronize: 'Appliquer la configuration',
    confirm_removal: 'Terminer le retrait',
  },
  gnomeManagementReceipts: {
    update:
      'Configuration mise à jour. Les terminaux ouverts restent inchangés.',
    rollback:
      'Configuration précédente restaurée. Les terminaux ouverts restent inchangés.',
    remove:
      'Configuration retirée. Terminez le retrait avant de relancer GNOME Terminal.',
    uninstall: 'Intégration retirée. Les terminaux ouverts restent inchangés.',
    recover_install:
      'Installation préparée. Appliquez la configuration avant le prochain démarrage de GNOME Terminal.',
    finish_update:
      'Mise à jour terminée. Les terminaux ouverts restent inchangés.',
    restore_update:
      'Configuration précédente restaurée. Les terminaux ouverts restent inchangés.',
    recover_removal:
      'Configuration retirée. Terminez le retrait avant de relancer GNOME Terminal.',
    synchronize:
      'Configuration appliquée aux prochains démarrages de GNOME Terminal. Les terminaux ouverts restent inchangés.',
    confirm_removal:
      'Intégration retirée. Les terminaux ouverts restent inchangés.',
  },
  gnomeManaging: 'En cours…',
  gnomeManagementFailed:
    'L’opération n’a pas pu aboutir. Vérifiez l’état actualisé avant de réessayer.',
  gnomeManagementStale:
    'La configuration a changé. Vérifiez l’état actualisé avant de réessayer.',
  gnomeManagementRefreshFailed:
    'L’état n’a pas pu être actualisé. Actualisez-le avant toute nouvelle action.',
  gnomeActivationPhase: {
    absent: 'Non installé.',
    configured: 'Configuration de Lyn vérifiée.',
    installing: 'Installation à terminer.',
    finishing_update: 'Mise à jour à terminer.',
    restoring_update: 'Configuration précédente à restaurer.',
    removing: 'Désinstallation à terminer.',
    awaiting_removal_confirmation: 'Retrait à confirmer.',
    changed: 'Configuration modifiée ; vérification nécessaire.',
    unavailable: 'État de l’installation indisponible.',
  },
  integrationDescriptions: {
    gnome: 'Associe les captures au projet du terminal GNOME local actif.',
    cursor:
      'Transmet le dossier de travail Cursor actif à Lyn lors de la capture.',
    vscode:
      'Transmet le dossier de travail VS Code actif à Lyn lors de la capture.',
    browser:
      'Capture depuis l’onglet focalisé ; localhost n’est lié à un projet que si un seul répertoire de processus est prouvé.',
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
  modelApproxSize: 'Environ 942 Mo',
  voiceCaption: 'Légende vocale',
  voiceCaptionLimit:
    '500 caractères maximum. Votre correction est prioritaire sur la transcription.',
  voiceCaptionSaving: 'Enregistrement…',
  voiceCaptionSave: 'Enregistrer la légende',
  voiceCaptionCancel: 'Annuler',
  voiceCaptionGenerated: 'Légende générée',
  voiceCaptionEdit: 'Modifier la légende',
  voiceTranscribing: 'Transcription sur cet appareil…',
  voiceTranscriptionFailed:
    'La transcription a échoué. Votre enregistrement est sauvegardé.',
  voiceTranscriptionRetry: 'Réessayer la transcription',
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
  deleteProject: 'Supprimer le projet',
  confirmProjectDelete: (name) =>
    `Retirer « ${name} » de la Library ? Son dossier restera sur le disque.`,
  projectNotEmpty:
    'Ce projet contient encore des captures. Supprimez-les d’abord.',
  projectDeleteFailed: 'Le projet n’a pas pu être supprimé. Réessayez.',
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
