// All texts shown in Eifel Mastering Central. To add a language later, copy this file (e.g. de.ts) and translate
// the values; the keys stay the same. {name} style placeholders are filled in by t().

export const en = {
  "app.wordmark.light": "Eifel Mastering",
  "app.wordmark.bold": "CENTRAL",
  "app.subtitle": "Plug-in manager · all Recky's plug-ins in one place",

  "header.updates": "Updates",
  "header.ready": "{n} READY",
  "header.updateAll": "Update all",
  "header.settings": "Settings",

  "filter.all": "All",
  "filter.installed": "Installed",
  "filter.updates": "Updates",
  "filter.new": "New",
  "filter.section": "Plug-ins",
  "filter.search": "Search…",

  "unit.installed": "Installed",
  "unit.installedNew": "Installed · new",
  "unit.available": "Available",
  "status.updateReady": "Update ready",
  "status.upToDate": "Up to date",
  "status.notInstalled": "Not installed",
  "status.new": "New",
  "status.notForPlatform": "Not available for {platform}",
  "button.update": "Update",
  "button.install": "Install",
  "button.options": "Options ▾",
  "options.reinstall": "Reinstall",
  "options.remove": "Remove",
  "options.whatsNew": "What's new in {version}",

  "list.empty.filter": "No plug-ins match this filter.",
  "list.empty.catalog": "The catalog contains no plug-ins yet.",
  "list.loading": "Loading the catalog…",

  "footer.catalogOk": "Catalog up to date · checked {when}",
  "footer.catalogCached": "Offline · showing the catalog from {when}",
  "footer.catalogError": "Catalog not reachable",
  "footer.catalogLoading": "Checking the catalog…",
  "footer.dawInactive": "DAW check comes in a later build",
  "footer.version": "Central {version}",

  "when.justNow": "just now",
  "when.minutes": "{n} min ago",
  "when.hours": "{n} h ago",
  "when.date": "{date}",

  "banner.offline": "The catalog could not be loaded. Check your internet connection.",
  "banner.retry": "Retry",
  "banner.schema": "This catalog needs a newer Eifel Mastering Central. Please update Central.",

  "settings.title": "Settings",
  "settings.formatsMac": "Formats (Mac)",
  "settings.formatsWin": "Formats (Windows)",
  "settings.location": "Install location",
  "settings.locationSystem": "System (all users)",
  "settings.locationUser": "Only my user",
  "settings.updates": "Updates",
  "settings.checkAtStart": "Check at start",
  "settings.catalog": "Catalog",
  "settings.reachable": "Reachable",
  "settings.unreachable": "Not reachable",
  "settings.uninstall": "Uninstall",
  "settings.uninstallAll": "Uninstall all",
  "settings.uninstallNote": "Asks first · one password prompt for all",
  "settings.central": "Central",
  "settings.close": "Close",
  "settings.formatNeeded": "At least one format must stay on.",

  "toast.notYet": "Installing and removing come in the next build of Central. This build only shows the catalog.",

  "platform.macos": "Mac",
  "platform.windows": "Windows",
} as const;

export type TextKey = keyof typeof en;
