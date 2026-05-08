window.RSCApp = window.RSCApp || {};

(function (app) {
  const RSC = window.RSC_CONSTANTS;
  const configJsonTypes = new Set(RSC.messages.configJsonTypes);

  app.state = app.state || {
    socket: null,
    commandHistory: window.commands || [],
    lastLogLineCount: window.lastLogLineCount || {},
  };
  window.commands = app.state.commandHistory;
  window.lastLogLineCount = app.state.lastLogLineCount;

  app.encodeSocketMessage = function (obj) {
    return configJsonTypes.has(obj.type)
      ? JSON.stringify(obj)
      : window.MessagePack.encode(obj);
  };

  app.createEvent = function (type, args) {
    return app.encodeSocketMessage({
      type: type ?? [],
      arguments: args ?? [],
    });
  };

  app.sendSocketMessage = function (obj) {
    if (!app.state.socket || app.state.socket.readyState !== WebSocket.OPEN) {
      return false;
    }
    app.state.socket.send(app.encodeSocketMessage(obj));
    return true;
  };

  app.decodeWebSocketMessage = function (message) {
    if (typeof message.data === "string") {
      return JSON.parse(message.data);
    }
    if (message.data instanceof ArrayBuffer) {
      return window.MessagePack.decode(new Uint8Array(message.data));
    }
    console.warn("[WebSocket] Received unknown message type:", message);
    return null;
  };

  app.getWebSocketAddress = function () {
    return (
      document.location.href
        .replace("http", "ws")
        .replace("https", "wss")
        .replace("#", "") + "ws"
    );
  };

  app.hotReloadWhenReady = function () {
    setInterval(function () {
      try {
        const req = new XMLHttpRequest();
        req.onreadystatechange = function () {
          if (this.status === 200) {
            document.location.reload();
          }
        };
        req.open("GET", document.location.href);
        req.send();
      } catch (e) {}
    }, RSC.animation.reconnectMs);
  };

  app.setSocket = function (socket) {
    app.state.socket = socket;
    window.socket = socket;
  };

  app.hasPermission = function (permission) {
    const auth = app.state.auth;
    if (!auth || !Array.isArray(auth.permissions)) return false;
    return auth.permissions.includes("admin") || auth.permissions.includes(permission);
  };

  app.hasServerPermission = function (server, permission) {
    const auth = app.state.auth;
    if (!auth || !Array.isArray(auth.permissions)) return false;
    if (auth.permissions.includes("admin") || auth.permissions.includes(permission)) return true;
    const ids = [server?.server_uuid, server?.name].filter(Boolean);
    return ids.some((id) => auth.permissions.includes(`server:${id}:${permission}`));
  };

  app.hasAnyServerPermission = function (permission) {
    const auth = app.state.auth;
    if (!auth || !Array.isArray(auth.permissions)) return false;
    if (auth.permissions.includes("admin") || auth.permissions.includes(permission)) return true;
    return auth.permissions.some(
      (value) => value.startsWith("server:") && value.endsWith(`:${permission}`),
    );
  };

  app.authRequest = async function (path, body) {
    const response = await fetch(path, {
      method: body ? "POST" : "GET",
      headers: body ? { "Content-Type": "application/json" } : {},
      credentials: "same-origin",
      body: body ? JSON.stringify(body) : undefined,
    });
    if (!response.ok) {
      let message = "Authentication failed";
      try {
        const error = await response.json();
        if (error?.error) message = error.error;
      } catch (e) {}
      throw new Error(message);
    }
    if (response.status === 204) return null;
    return response.json();
  };
})(window.RSCApp);
