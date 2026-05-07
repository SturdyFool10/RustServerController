$(document).ready(function () {
  const app = window.RSCApp;
  const RSC = window.RSC_CONSTANTS;
  const socket = new WebSocket(app.getWebSocketAddress());

  app.setSocket(socket);
  socket.binaryType = "arraybuffer";
  socket.onerror = app.hotReloadWhenReady;
  socket.onclose = app.hotReloadWhenReady;
  socket.addEventListener("open", function () {
    socket.send(app.createEvent(RSC.messages.requestInfo, [true]));
    app.requestThemesList();
  });

  socket.onmessage = function (message) {
    let obj;
    try {
      obj = app.decodeWebSocketMessage(message);
    } catch (e) {
      console.error("[WebSocket] Could not decode message:", e);
      return;
    }
    if (!obj) return;

    switch (obj.type) {
      case "ConfigInfo":
        app.setEditorConfig(obj.config);
        app.updateAdministration?.();
        break;
      case "ServerInfo":
        app.mergeServerInfoSnapshot(obj);
        app.updateServerInfoSpecializations();
        app.updateAdministration?.();
        break;
      case "ServerSpecializationInfoUpdate":
        app.mergeSpecializationInfoUpdate(obj);
        app.updateServerInfoSpecializations();
        break;
      case "ServerOutput":
        app.processServerLogLines(obj.server_name, obj.output, false);
        break;
      case "themesList":
        app.handleThemesList(obj.themes);
        break;
      case "themeCSS":
        app.applyTheme(obj.theme_name, obj.css);
        break;
    }
  };

  app.loadThemeFromStorage();
  app.initConfigEditor();
  app.initNavigation();
  app.initStats();
  app.initAdministration();
  app.initWebglBackground();
});
