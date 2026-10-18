
# RustServerController Documentation

## Current Project Status:
### Main Branch:
- Windows Build Status: [![Build Status for Windows](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_win.yml/badge.svg?branch=main)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_win.yml)
- Linux Build Status: [![Build Status for Linux](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_linux.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_linux.yml)
- MacOS Build Status: [![Build Status for MacOS](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_MacOS.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_MacOS.yml)

### Testing Branch:
- Windows Testing Build Status: [![Build Status for Windows Testing](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_win_testing.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_win_testing.yml)
- Linux Testing Build Status: [![Build Status for Linux Testing](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_linux_testing.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_linux_testing.yml)
- MacOS Testing Build Status: [![Build Status for MacOS Testing](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_MacOS_testing.yml/badge.svg)](https://github.com/SturdyFool10/RustServerController/actions/workflows/build_MacOS_testing.yml)

RustServerController is designed for seamless server management on remote machines, with minimal resource usage, ensuring it remains unobtrusive in most scenarios.

## Building the Project:
You can start building in two ways:

### Building from Source:
- Clone the main branch of the repository.
- Navigate to the root directory, which mirrors the structure of the GitHub repository's main branch.
- Open your terminal and execute the following command:
  ```bash
  cargo build --release
  ```
- Once built, the compiled executable can be found at `/target/release/server_host.exe`. This file is standalone and can be moved as needed.
- Note: Ensure that you have installed Cargo from [rustup.rs](http://rustup.rs). Git might also be required for cloning the repository.

### Using Pre-Compiled Executables:
- Pre-compiled executables are regularly updated and posted in the repository's releases tab.
- These executables are less secure for those familiar with Rust, as source code may not always be provided, but they are easier to use and require less technical knowledge.

# Multi-Node (Master and Nodes)

One controller (the **master**) runs the web UI and owns the configuration for the whole network. Other machines run a controller in **node** mode and just execute the servers the master assigns to them. In the web UI a server on a node looks and behaves exactly like a local one: console, stdin, start/stop/restart, status, per-game stats and player activity (merged across nodes) all work the same, and you edit everything in the master's config.

## Setting up a node
Set `"slave": true` in the node's `config.json` and give it an OAuth client for the master to log in with. The `client_secret_hash` is the SHA-256 of the secret:

```sh
printf %s 'a-long-random-secret' | sha256sum
```

```json
{
  "slave": true,
  "interface": "0.0.0.0",
  "port": "8081",
  "auth": {
    "oauth_clients": [
      {
        "client_id": "master",
        "client_secret_hash": "<sha256 from above>",
        "scopes": ["admin"]
      }
    ]
  }
}
```

A node hosts no web UI, and the servers in its own `servers` list are replaced by whatever the master assigns, so don't configure servers on it by hand.

## Pointing the master at a node
Add an entry to the master's `slave_connections`, then give each server that should run there a `node`:

```json
{
  "slave_connections": [
    {
      "name": "east",
      "address": "10.0.0.5",
      "port": "8081",
      "use_https": true,
      "pinned_certificate_sha256": "<optional, SHA-256 fingerprint of the node's certificate>",
      "client_id": "master",
      "client_secret": "a-long-random-secret"
    }
  ],
  "servers": [
    { "name": "survival", "specialized_server_type": "Minecraft", "node": "east", "...": "..." },
    { "name": "lobby", "...": "no node: runs on the master itself" }
  ]
}
```

- `name` is what servers reference in `node` (default: `address:port`).
- Servers are identified by their `server_uuid` (generated automatically; a duplicated UUID in the config is replaced). Server names are only display labels, so they may repeat or change without breaking anything: console, stats, permissions, backups of player history and node assignment all follow the UUID. Clients and API users should send `server_uuid` in `stdinInput`, `startServer`, `killServer` and `restartServer` messages (`server_name` is only used when no UUID is given).
- Add, remove, or move servers between nodes by editing the master's config; the master pushes each node its servers (and the account filter groups) whenever the config changes or the node reconnects. Moving a server to a different node starts a fresh process there, so move the world files too.
- The master keeps reconnecting if a node is down. Its servers show as offline until it's back; nothing runs on the master in the meantime.
- Changes to the `slave_connections` list itself need a master restart.
- `client_secret` is stored in plain text in the master's config (only admins can see it in the UI), so protect that file and prefer `use_https` with a pinned certificate.
- Chaining masters is not supported.
---

# Server Process JSON Structure

A server process in RustServerController is described in the configuration file as a JSON object. This object defines how the controller should launch and manage the server. Below is an example of what a typical server process entry looks like in JSON:

```json
{
  "name": "<namehereNOSPACES>",
  "exe_path": "<binary_path_here>",
  "arguments": ["insert", "args", "comma", "seperated"],
  "working_dir": "<workingDirHere>",
  "auto_start": true,
  "crash_prevention": true,
  "specialized_server_type": null,
  "specialization_options": null
}
```

## Field Descriptions

- **name**: A friendly name for your server process. avoid using spaces in the name.
- **exe_path**: The path to the executable or script to run (e.g., a `.exe` on windows or an executable on linux).
- **arguments**: An array of command-line arguments to pass to the executable. these use json syntax and are expected to be strings(google it if you don't know it).
- **working_dir**: The working directory from which the process will be launched. in the case of a minecraft server for example, this will be the folder where the server instance stores all of its files.
- **auto_start**: If `true`, the server will start automatically when the controller launches.
- **crash_prevention**: If `true`, the controller will attempt to restart the server if it crashes.
- **specialized_server_type(Optional, Default: null)**: allows the user to specify what type of server they are running for extra features, Currently Supported Values: "Minecraft", "Hytale", "Terraria"(does nothing yet), "VintageStory", null
- **specialization_options(Optional, Default: null)**: a JSON object holding settings specific to whatever `specialized_server_type` is chosen. You only need to set the keys you want to change — anything you leave out is filled in automatically with that specialization's defaults (and the file gets rewritten with the full merged object the first time the controller loads it). See [Specialization Options](#specialization-options) below for what each specialization type supports.
> **Note:**
> Advanced fields like `specialized_server_info` are managed internally by RustServerController and should not be set or modified manually in your configuration. Most users will never need to use or change this field.

You can add multiple server process objects to the `servers` array in your configuration file to manage several servers at once.

---

# Specialization Options

Setting `specialized_server_type` unlocks a `specialization_options` object with extra, type-specific settings. Every field is optional; anything you don't set uses the default shown below.

## Hytale

```json
{
  "specialized_server_type": "Hytale",
  "specialization_options": {
    "controller_controlled_whitelist": false,
    "controller_controlled_ban_list": false,
    "account_filter_groups": []
  }
}
```

The Hytale specialization keeps the server's ANSI console colors, tracks players joining/leaving (including the per-player activity stats, which follow a player's UUID: if a UUID joins under a new name, its history on that server is migrated to the new name), reads `MaxPlayers` from the server's `config.json`, and shows whether the server has finished booting.

- **controller_controlled_whitelist** / **controller_controlled_ban_list** / **account_filter_groups**: work like the Minecraft options below, using the same account filter groups. Once the server has booted (or 90 seconds have passed), the controller sends `/whitelist enable`, `/whitelist add <name>` and `/ban <name>` through the console, then sends only the changes (`/whitelist remove`, `/unban`) as you edit the groups. Hytale has no IP bans, so IP ban entries are ignored. Entries are sent by player name, nothing is imported back from the server's `whitelist.json`/`bans.json`, and an entry you remove while the server is offline stays on the server until you remove it in-game.
- Backups aren't offered because Hytale has its own built-in backup system.

## Minecraft

```json
{
  "specialized_server_type": "Minecraft",
  "specialization_options": {
    "auto_accept_eula": true,
    "controller_controlled_whitelist": false,
    "controller_controlled_ban_list": false,
    "account_filter_groups": [],
    "backup": {
      "enabled": false,
      "interval": "6h",
      "world_folder": null,
      "backup_dir": "backups",
      "compression": "xz",
      "compression_level": 6,
      "retention": "14days",
      "max_disk_size": null,
      "announce_message": "Backing up server world..."
    }
  }
}
```

- **auto_accept_eula**: If `true`, RustServerController automatically flips `eula=false` to `eula=true` in `eula.txt` and restarts the server for you the first time it exits over an unaccepted EULA.
- **controller_controlled_whitelist**: If `true`, the controller manages this server's `whitelist.json` and `white-list` server-property for you, based on the account filter group(s) below. Leave `false` if you manage the whitelist yourself in-game or by hand.
- **controller_controlled_ban_list**: Same idea as above, but for `banned-players.json` and `banned-ips.json`.
- **account_filter_groups**: A list of account filter group UUIDs (see the top-level `minecraft_account_filter_detail_groups` config array) whose whitelist/ban entries should apply to this server. Only used when one of the two `controller_controlled_*` flags above is `true`.
- **backup**: Scheduled, fault-tolerant world backups. See below.

### Minecraft World Backups (`backup`)

- **enabled** (`bool`, default `false`): Turns scheduled backups on or off for this server.
- **interval** (duration string, default `"6h"`): How often a backup is taken. Accepts human-friendly durations like `"30m"`, `"6h"`, `"1d"`, `"2w"` (powered by [humantime](https://docs.rs/humantime) — you can also combine units, e.g. `"1h 30m"`).
- **world_folder** (string, list of strings, or `null`, default `null`): The world folder(s) to back up. Each one is resolved relative to `working_dir` (the instance's CWD) unless you give it as an absolute path.
  - Leave as `null` to auto-detect from `level-name` in `server.properties` (falls back to Minecraft's own default, `"world"`, if that's missing too).
  - Set a single string (e.g. `"world"`) to back up just that folder.
  - Set a list (e.g. `["world", "world_nether", "world_the_end"]`) to bundle multiple folders — useful for the Nether/End dimension folders vanilla Minecraft keeps alongside the overworld — into a single backup archive.
- **backup_dir** (string, default `"backups"`): Where backup archives are written, relative to `working_dir` (or an absolute path if you provide one).
- **compression** (string, default `"xz"`): Compression algorithm for the archive. One of:
  - `"none"` — plain, uncompressed `.tar`
  - `"gzip"` — `.tar.gz`
  - `"zstd"` — `.tar.zst`
  - `"xz"` — `.tar.xz` (smallest files, slowest to compress)
- **compression_level** (integer, default `6`): How hard to compress. The valid range depends on `compression` and out-of-range values are simply clamped: `gzip` is `0`–`9`, `zstd` is `1`–`22`, `xz` is `0`–`9`. Ignored when `compression` is `"none"`.
- **retention** (duration string, default `"14days"`): How long a backup is kept before it becomes eligible for automatic deletion. Same duration syntax as `interval`.
- **max_disk_size** (human-readable size string, or `null`, default `null`): Caps total disk space used by this server's backups. Accepts decimal units (`"5GB"`, `"750MB"`, base 1000) and binary units (`"5GiB"`, `"750MiB"`, base 1024) — `KB`/`KiB`, `MB`/`MiB`, `GB`/`GiB`, `TB`/`TiB`, etc. all work, and a plain number is treated as a raw byte count. Once exceeded, the oldest backups are deleted first. `null` means unlimited. The single newest backup is never deleted this way, so a quota set smaller than one backup's size can't wipe out your only copy.
- **announce_message** (string, default `"Backing up server world..."`): A chat message sent via the server's `say` command right before a backup starts, so players know why the server might briefly stutter. Set it to `""` (or all whitespace) to disable the announcement entirely.
  - Supports substitution tokens you can mix into your own message: `{server_name}`, `{world_folder}` (the world folder(s) actually being backed up, comma-separated), `{date}` (UTC `YYYY-MM-DD`), `{time}` (UTC `HH:MM:SS`), and `{datetime}` (UTC `YYYY-MM-DD HH:MM:SS`). For example: `"[{server_name}] Backing up {world_folder} at {time}, hang tight!"`.
  - The final message is always sanitized before being sent: every control character (including newlines) is stripped, since an embedded newline in a message is what could otherwise smuggle a second, attacker-chosen line of input into the server's console and have it executed as its own command. This applies to both your template text and any substituted value (e.g. a server name), so a crafted server name can't be used to inject extra commands through this feature.

Backups are written to a temporary file first and only renamed into place once fully written, so a crash or power loss mid-backup can never leave a corrupt/partial archive behind, and a restarted controller checks the newest backup already on disk (rather than any in-memory timer) to decide whether one is due — so scheduling survives restarts correctly.
