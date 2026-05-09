# Controller Plugins

Controller plugins are installable controller-side mods. Each plugin lives in its
own directory under `controller_plugins` and provides a `manifest.json`.

Example:

```json
{
  "id": "factorio",
  "name": "Factorio",
  "version": "0.1.0",
  "enabled": true,
  "description": "Adds Factorio-specific controller behavior.",
  "capabilities": ["specialization", "frontend"],
  "frontend": {
    "modules": ["ui.js"],
    "styles": ["ui.css"]
  },
  "backend": {
    "wasm_module": "backend.wasm"
  },
  "specializations": [
    {
      "name": "Factorio",
      "display_name": "Factorio",
      "description": "Factorio server support",
      "default_options": {
        "rcon_port": 27015
      },
      "status": {},
      "stats": {}
    }
  ]
}
```

Frontend modules are loaded after authentication. A module can register a richer
server UI:

```js
window.RSCApp.registerServerSpecialization("Factorio", {
  updateUI(dropdownElement, server) {
    const title = dropdownElement.querySelector(".serverName");
    if (title) title.textContent = `${server.name} (Factorio)`;
  },
});
```

Only assets declared in the manifest are served. Backend WASM modules are loaded
without WASI imports, so they cannot directly access files, sockets, processes,
or environment variables.

## Backend WASM ABI

WASM modules must export:

- `memory`
- `rsc_alloc(len: i32) -> i32`
- optional `rsc_dealloc(ptr: i32, len: i32)`

Hook functions receive a UTF-8 JSON buffer as `(ptr: i32, len: i32)` and return
an `i64` where the high 32 bits are the output pointer and the low 32 bits are
the output length.

Optional hooks:

- `rsc_default_options(input_json) -> json`
- `rsc_status(input_json) -> json`
- `rsc_stats(input_json) -> json`
- `rsc_parse_output(input_json) -> json`

`rsc_parse_output` receives:

```json
{
  "plugin_id": "factorio",
  "specialization": "Factorio",
  "server": "Main",
  "server_uuid": "...",
  "line": "server output line",
  "options": {}
}
```

It may return:

```json
{
  "line": "modified output line",
  "status_update": true,
  "status": {}
}
```

Use `"line": null` to hide an output line. Missing hooks fall back to the
manifest behavior.
