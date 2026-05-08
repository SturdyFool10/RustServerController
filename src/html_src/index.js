$(document).ready(async function () {
  const app = window.RSCApp;
  const RSC = window.RSC_CONSTANTS;

  function authElements() {
    return {
      screen: document.getElementById("auth-screen"),
      form: document.getElementById("auth-form"),
      mode: document.getElementById("auth-mode"),
      username: document.getElementById("auth-username"),
      password: document.getElementById("auth-password"),
      passwordRepeatField: document.getElementById("auth-password-repeat-field"),
      passwordRepeat: document.getElementById("auth-password-repeat"),
      error: document.getElementById("auth-error"),
      submit: document.getElementById("auth-submit"),
      requestAccount: document.getElementById("auth-request-account"),
      main: document.getElementById("main-content"),
      menu: document.getElementById("menu"),
    };
  }

  function bytesToBase64(bytes) {
    let binary = "";
    bytes.forEach((byte) => {
      binary += String.fromCharCode(byte);
    });
    return btoa(binary);
  }

  function bytesToHex(bytes) {
    return Array.from(bytes)
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("");
  }

  async function derivePasswordHash(password, salt) {
    const encoder = new TextEncoder();
    const key = await crypto.subtle.importKey(
      "raw",
      encoder.encode(password),
      "PBKDF2",
      false,
      ["deriveBits"],
    );
    const bits = await crypto.subtle.deriveBits(
      {
        name: "PBKDF2",
        salt: encoder.encode(salt),
        iterations: 310000,
        hash: "SHA-256",
      },
      key,
      256,
    );
    return bytesToBase64(new Uint8Array(bits));
  }

  async function challengeProof(passwordHash, nonce) {
    const encoder = new TextEncoder();
    const digest = await crypto.subtle.digest(
      "SHA-256",
      encoder.encode(`${passwordHash}:${nonce}`),
    );
    return bytesToHex(new Uint8Array(digest));
  }

  async function createSetupPayload(username, password) {
    const saltBytes = new Uint8Array(32);
    crypto.getRandomValues(saltBytes);
    const password_salt = bytesToBase64(saltBytes);
    return {
      username,
      password_salt,
      password_hash: await derivePasswordHash(password, password_salt),
    };
  }

  async function createLoginPayload(username, password) {
    const challenge = await app.authRequest("/auth/challenge", { username });
    if (challenge.password_required) {
      return {
        username,
        nonce: challenge.nonce,
      };
    }
    if (!challenge.password_salt) {
      throw new Error("invalid username or password");
    }
    const passwordHash = await derivePasswordHash(password, challenge.password_salt);
    return {
      username,
      nonce: challenge.nonce,
      proof: await challengeProof(passwordHash, challenge.nonce),
    };
  }

  function validatePasswordPair(password, repeated) {
    if (password.length < 8) {
      throw new Error("password must be at least 8 characters");
    }
    if (password !== repeated) {
      throw new Error("passwords do not match");
    }
  }

  function showLogin(status) {
    const els = authElements();
    const setup = !!status?.setup_required;
    els.screen.hidden = false;
    els.main.hidden = true;
    els.menu.hidden = true;
    els.mode.textContent = setup ? "Create the first administrator" : "Sign in";
    els.submit.textContent = setup ? "Create administrator" : "Sign in";
    els.username.disabled = false;
    els.password.autocomplete = setup ? "new-password" : "current-password";
    els.password.required = true;
    els.passwordRepeatField.hidden = !setup;
    els.passwordRepeat.required = setup;
    els.requestAccount.hidden = setup;
    els.error.textContent = "";
    app.requestThemesList?.();
    els.requestAccount.onclick = async function () {
      els.requestAccount.disabled = true;
      els.error.textContent = "";
      try {
        const username = els.username.value.trim();
        if (!username) throw new Error("username is required");
        await app.authRequest("/auth/request-account", { username });
        els.error.textContent = "Account request sent";
      } catch (error) {
        els.error.textContent = error.message;
      } finally {
        els.requestAccount.disabled = false;
      }
    };
    els.form.onsubmit = async function (event) {
      event.preventDefault();
      els.submit.disabled = true;
      els.error.textContent = "";
      try {
        const username = els.username.value.trim();
        const password = els.password.value;
        if (setup) {
          validatePasswordPair(password, els.passwordRepeat.value);
        }
        const payload = setup
          ? await createSetupPayload(username, password)
          : await createLoginPayload(username, password);
        const auth = await app.authRequest(setup ? "/auth/setup" : "/auth/login", payload);
        els.password.value = "";
        els.passwordRepeat.value = "";
        if (auth.password_required) {
          showSetPassword(auth);
          return;
        }
        await startAuthenticatedApp(auth);
      } catch (error) {
        els.error.textContent = error.message;
      } finally {
        els.submit.disabled = false;
      }
    };
  }

  function showSetPassword(auth) {
    const els = authElements();
    els.screen.hidden = false;
    els.main.hidden = true;
    els.menu.hidden = true;
    els.mode.textContent = "Set your password";
    els.submit.textContent = "Set password";
    els.username.value = auth.username || "";
    els.username.disabled = true;
    els.password.value = "";
    els.password.autocomplete = "new-password";
    els.password.required = true;
    els.passwordRepeatField.hidden = false;
    els.passwordRepeat.value = "";
    els.passwordRepeat.required = true;
    els.requestAccount.hidden = true;
    els.error.textContent = "";
    app.requestThemesList?.();
    els.form.onsubmit = async function (event) {
      event.preventDefault();
      els.submit.disabled = true;
      els.error.textContent = "";
      try {
        const password = els.password.value;
        validatePasswordPair(password, els.passwordRepeat.value);
        const payload = await createSetupPayload(auth.username, password);
        const updatedAuth = await app.authRequest("/auth/set-password", {
          password_salt: payload.password_salt,
          password_hash: payload.password_hash,
        });
        els.username.disabled = false;
        els.password.value = "";
        els.passwordRepeat.value = "";
        await startAuthenticatedApp(updatedAuth);
      } catch (error) {
        els.error.textContent = error.message;
      } finally {
        els.submit.disabled = false;
      }
    };
  }

  function hideLogin() {
    const els = authElements();
    els.screen.hidden = true;
    els.main.hidden = false;
    els.menu.hidden = false;
  }

  function openSocket() {
    const socket = new WebSocket(app.getWebSocketAddress());

    app.setSocket(socket);
    socket.binaryType = "arraybuffer";
    socket.onerror = app.hotReloadWhenReady;
    socket.onclose = app.hotReloadWhenReady;
    socket.addEventListener("open", function () {
      socket.send(app.createEvent(RSC.messages.requestInfo, [true]));
      app.requestThemesList();
    });

    socket.onmessage = handleSocketMessage;
  }

  function handleSocketMessage(message) {
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
      case "AuthRequired":
        window.location.reload();
        break;
    }
  }

  async function startAuthenticatedApp(auth) {
    app.state.auth = auth;
    hideLogin();
    openSocket();
    app.initConfigEditor();
    app.initNavigation();
    app.initStats();
    app.initAdministration();
    app.initWebglBackground();
  }

  app.loadThemeFromStorage();
  try {
    const status = await app.authRequest("/auth/status");
    if (status.authenticated) {
      if (status.password_required) {
        showSetPassword(status);
      } else {
        await startAuthenticatedApp(status);
      }
    } else {
      showLogin(status);
    }
  } catch (error) {
    showLogin({ setup_required: false });
  }
});
