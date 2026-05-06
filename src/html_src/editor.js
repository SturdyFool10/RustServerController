window.RSCApp = window.RSCApp || {};

(function (app) {
  const RSC = window.RSC_CONSTANTS;
  let editor = null;

  function setEditorConfig(config) {
    window.config = config;
    const jsonConfig = JSON.stringify(config, undefined, 4);
    $(".editorText").val(jsonConfig);
    if (editor) {
      editor.setValue(jsonConfig);
    }
  }

  function initMonacoEditor() {
    if (typeof monaco === "undefined") return;

    monaco.editor.defineTheme("rustController", {
      base: "vs-dark",
      inherit: true,
      rules: [
        { token: "string", foreground: "50fa7b" },
        { token: "number", foreground: "ff79c6" },
        { token: "keyword", foreground: "bd93f9" },
      ],
      colors: {
        "editor.background": "#1e1e1e",
        "editor.foreground": "#f8f8f2",
        "editor.lineHighlightBackground": "#282a36",
        "editorLineNumber.foreground": "#6272a4",
        "editorLineNumber.activeForeground": "#f8f8f2",
        "editorCursor.foreground": "#f8f8f2",
        "editor.selectionBackground": "#44475a",
        "editor.inactiveSelectionBackground": "#44475a80",
      },
    });

    editor = monaco.editor.create(document.getElementById("jsonEditor"), {
      value: $(".editorText").val() || "{}",
      language: "json",
      theme: "rustController",
      automaticLayout: true,
      formatOnPaste: true,
      formatOnType: true,
      minimap: {
        enabled: true,
        maxColumn: 80,
        renderCharacters: true,
        scale: 1,
        showSlider: "always",
      },
      scrollBeyondLastLine: false,
      tabSize: 4,
      insertSpaces: true,
      wordWrap: "off",
      lineNumbers: "on",
      fontLigatures: true,
      fontFamily:
        "'Fira Code', 'JetBrains Mono', 'Consolas', 'Courier New', monospace",
    });

    editor.onDidChangeModelContent(function () {
      $(".editorText").val(editor.getValue());
    });
  }

  function sendRequestConfig() {
    if (app.state.socket && app.state.socket.readyState === WebSocket.OPEN) {
      app.state.socket.send(JSON.stringify({ type: RSC.messages.requestConfig }));
    } else {
      setTimeout(sendRequestConfig, RSC.animation.retryMs);
    }
  }

  function bindSaveButton() {
    $(".configSave").click(function (e) {
      if (e.which !== 1) return;

      try {
        const jsonContent = editor ? editor.getValue() : $(".editorText").val();
        const newConfig = JSON.parse(jsonContent);
        app.state.socket.send(
          JSON.stringify({
            type: RSC.messages.configChange,
            updatedConfig: newConfig,
          }),
        );

        const formattedJson = JSON.stringify(newConfig, null, 4);
        if (editor) {
          editor.setValue(formattedJson);
        } else {
          $(".editorText").val(formattedJson);
        }

        $(this).find(".bloom").css("opacity", "1");
        setTimeout(() => {
          $(this).find(".bloom").css("opacity", "0");
        }, RSC.animation.saveFeedbackMs);
      } catch (error) {
        alert("Invalid JSON: " + error.message);
      }
    });
  }

  app.initConfigEditor = function () {
    window.config = { state: "NotInit" };
    require(["vs/editor/editor.main"], initMonacoEditor);
    bindSaveButton();
    sendRequestConfig();
  };

  app.setEditorConfig = setEditorConfig;
})(window.RSCApp);
