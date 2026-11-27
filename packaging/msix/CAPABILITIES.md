# Larafim: restricted capabilities (Microsoft Store)

Full justification for the restricted capabilities declared in the MSIX package
([AppxManifest.xml](AppxManifest.xml)). Larafim is a free, open-source local development tool for
Windows (similar to Laravel Herd or Laragon). Source code: https://github.com/josevictoremiliano/larafim

## runFullTrust

Larafim is a free, open-source desktop developer tool for Windows (Tauri/WebView2), similar to Laravel Herd or Laragon. It installs PHP, Node.js and databases for the user's local web projects and starts them as local processes (PHP-FastCGI, MariaDB, PostgreSQL, Redis, Mailpit), plus a local HTTP gateway listening only on 127.0.0.1. This requires a full-trust Win32 process. Source code and support: https://github.com/josevictoremiliano/larafim

## unvirtualizedResources

**Why.** Larafim's purpose is to make the runtimes it installs usable from the user's own terminal (PowerShell, Windows Terminal, VS Code), which runs OUTSIDE the package. Typing "php artisan serve" or "composer install" in any terminal must reach the PHP version Larafim installed.
1. Runtimes and settings live in %LOCALAPPDATA%\Larafim. Virtualized, they would go to the package's private LocalCache folder, which the terminal and the PHP/Composer processes it starts cannot see.
2. Only if the user turns on "Shortcuts in PATH" (off by default, Settings > Terminal), Larafim adds ONE folder, %LOCALAPPDATA%\Larafim\bin, to the user's PATH in HKCU\Environment, and removes it when turned off. Virtualized, this write is only visible inside the package, so terminals never find php/composer/node.

**Alternatives evaluated.** Keeping data in the package's private folder (terminal cannot see it); app execution aliases for php/node (processes run inside the package, so Composer and npm global caches/packages go to a virtualized location that differs from what every other tool uses); asking users to edit PATH by hand (error-prone).

**Least privilege.** Unvirtualized writes go only to: %LOCALAPPDATA%\Larafim\ (runtimes, settings, logs, local databases) and the "Path" value of HKCU\Environment (adding/removing exactly %LOCALAPPDATA%\Larafim\bin, every other entry preserved). No HKLM keys, no system PATH, no Windows services.

**Safeguards.** PATH change is opt-in and reversible from the same switch; user projects are never modified without confirmation; all input from projects and downloads is validated (HTTPS + SHA-256 for downloads), covered by automated tests with malicious input. No telemetry. Privacy: https://github.com/josevictoremiliano/larafim/blob/main/PRIVACY.md

**Support.** https://github.com/josevictoremiliano/larafim/issues

**Assurance.** I confirm this capability is used only for the purposes above and that Larafim writes, through it, only to the paths listed above.

## allowElevation

**Why.** Each local project gets its own address, `<project>.offline`. For the browser AND command-line tools (curl, PHP HTTP clients, Node scripts calling another local project) to resolve these names, they must be in the Windows hosts file, which only an administrator can write. The main app never runs elevated; it asks through the standard UAC prompt only when the list of project addresses changes, and nothing changes unless the user clicks Yes. If declined, Larafim keeps working and explains that .offline addresses will not resolve.

**Alternatives evaluated.** `*.localhost` (browsers resolve it, but the Windows resolver used by PHP, curl and CLI tools does not, so projects calling each other break); a local DNS server (needs MORE privilege: changes the network adapter's DNS and affects all traffic - rejected); asking users to edit hosts by hand (a mistake breaks name resolution for the whole machine).

**Least privilege.** The elevated helper writes only to: %SystemRoot%\System32\drivers\etc\hosts, replacing only the block between "# BEGIN LARAFIM" and "# END LARAFIM", which holds only `127.0.0.1 <name>` lines (all other lines kept as they are); and %SystemRoot%\System32\drivers\etc\hosts.larafim-backup, a one-time copy of the original, created only if missing. The helper is the same executable started with one fixed argument; it writes the block and exits.

**Safeguards.** every name is validated twice (in the app and again inside the elevated helper): lowercase letters, digits, hyphens and dots only, max 253 chars, max 2000 entries; anything else is rejected and nothing is written. The IP is always 127.0.0.1, so the helper cannot redirect real domains or inject arbitrary lines.

**Support.** https://github.com/josevictoremiliano/larafim/issues

**Assurance.** I confirm this capability is used only for the purpose above and that the elevated helper writes only to the two paths listed above.
