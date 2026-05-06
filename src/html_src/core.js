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
})(window.RSCApp);
