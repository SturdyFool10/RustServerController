window.RSCApp = window.RSCApp || {};

(function (app) {
  const RSC = window.RSC_CONSTANTS;

  function applyTheme(themeName, cssContent) {
    const existingThemeStyle = document.getElementById(
      RSC.selectors.currentThemeStyleId,
    );
    if (existingThemeStyle) {
      existingThemeStyle.remove();
    }

    const themeStyle = document.createElement("style");
    themeStyle.id = RSC.selectors.currentThemeStyleId;
    themeStyle.textContent = cssContent;
    document.head.appendChild(themeStyle);

    localStorage.setItem(RSC.themeStorage.selectedTheme, themeName);
    localStorage.setItem(RSC.themeStorage.themeCss, cssContent);

    const themeSelector = document.getElementById(RSC.selectors.themeSelectorId);
    if (themeSelector) {
      themeSelector.value = themeName;
      const loadingIndicator = document.querySelector(RSC.selectors.themeLoading);
      if (loadingIndicator) {
        loadingIndicator.style.display = "none";
      }
    }

    document.body.style.transition =
      "background-color 0.3s ease, color 0.3s ease";
    setTimeout(() => {
      document.body.style.transition = "";
    }, RSC.animation.themeTransitionMs);
  }

  function loadThemeFromStorage() {
    const themeName = localStorage.getItem(RSC.themeStorage.selectedTheme);
    const themeCSS = localStorage.getItem(RSC.themeStorage.themeCss);
    if (themeName && themeCSS) {
      applyTheme(themeName, themeCSS);
      return themeName;
    }
    return null;
  }

  async function requestThemesList() {
    if (app.state.socket && app.state.socket.readyState === WebSocket.OPEN) {
      app.state.socket.send(JSON.stringify({ type: RSC.messages.getThemesList }));
      return true;
    }
    try {
      const response = await fetch("/themes", { credentials: "same-origin" });
      if (!response.ok) return false;
      const payload = await response.json();
      handleThemesList(payload.themes);
      return true;
    } catch (error) {
      return false;
    }
  }

  function setThemeLoading(visible) {
    let loadingIndicator = document.querySelector(RSC.selectors.themeLoading);
    if (!loadingIndicator) {
      const themeContainer = document.querySelector(RSC.selectors.themeContainer);
      if (themeContainer) {
        loadingIndicator = document.createElement("span");
        loadingIndicator.className = RSC.selectors.themeLoading.slice(1);
        themeContainer.appendChild(loadingIndicator);
      }
    }

    if (loadingIndicator) {
      loadingIndicator.style.display = visible ? "inline" : "none";
      loadingIndicator.textContent = RSC.uiText.loading;
    }
  }

  async function requestThemeCSS(themeName) {
    if (app.state.socket && app.state.socket.readyState === WebSocket.OPEN) {
      app.state.socket.send(
        JSON.stringify({
          type: RSC.messages.getThemeCSS,
          theme_name: themeName,
        }),
      );
      setThemeLoading(true);
      return true;
    }
    setThemeLoading(true);
    try {
      const response = await fetch(`/themes/css?theme_name=${encodeURIComponent(themeName)}`, {
        credentials: "same-origin",
      });
      if (!response.ok) return false;
      const payload = await response.json();
      applyTheme(payload.theme_name || themeName, payload.css || "");
      return true;
    } catch (error) {
      setThemeLoading(false);
      return false;
    }
  }

  function themeMount() {
    const authScreen = document.getElementById("auth-screen");
    const authMount = document.getElementById("auth-theme-mount");
    if (authScreen && !authScreen.hidden && authMount) return authMount;
    return document.querySelector(RSC.selectors.innerTopBar);
  }

  function ensureThemeSelector() {
    let themeSelector = document.getElementById(RSC.selectors.themeSelectorId);
    let themeContainer = document.querySelector(RSC.selectors.themeContainer);
    const mount = themeMount();
    if (!themeSelector) {
      themeSelector = document.createElement("select");
      themeSelector.id = RSC.selectors.themeSelectorId;
      themeSelector.addEventListener("change", function () {
        requestThemeCSS(this.value);
      });
    }
    if (!themeContainer) {
      themeContainer = document.createElement("div");
      themeContainer.className = RSC.selectors.themeContainer.slice(1);
      const label = document.createElement("label");
      label.htmlFor = RSC.selectors.themeSelectorId;
      label.textContent = "Theme";
      themeContainer.appendChild(label);
      themeContainer.appendChild(themeSelector);
      const loadingIndicator = document.createElement("span");
      loadingIndicator.className = RSC.selectors.themeLoading.slice(1);
      loadingIndicator.textContent = RSC.uiText.loading;
      loadingIndicator.style.display = "none";
      themeContainer.appendChild(loadingIndicator);
    }
    if (mount && themeContainer.parentElement !== mount) {
      mount.appendChild(themeContainer);
    }
    themeSelector.replaceChildren();
    return themeSelector;
  }

  function handleThemesList(themes) {
    if (!Array.isArray(themes) || themes.length === 0) return;

    const themeSelector = ensureThemeSelector();
    themes.forEach((theme) => {
      const option = document.createElement("option");
      option.value = theme;
      option.textContent = theme;
      themeSelector.appendChild(option);
    });

    const currentTheme =
      localStorage.getItem(RSC.themeStorage.selectedTheme) || themes[0];
    themeSelector.value = currentTheme;
    requestThemeCSS(currentTheme);
  }

  app.applyTheme = applyTheme;
  app.loadThemeFromStorage = loadThemeFromStorage;
  app.requestThemesList = requestThemesList;
  app.handleThemesList = handleThemesList;
})(window.RSCApp);
