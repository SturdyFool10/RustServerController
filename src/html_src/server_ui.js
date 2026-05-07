window.RSCApp = window.RSCApp || {};

(function (app) {
  const RSC = window.RSC_CONSTANTS;

  class ServerSpecialization {
    updateUI(_dropdownElement, _server) {}
  }

  class MinecraftSpecialization extends ServerSpecialization {
    updateUI(dropdownElement, server) {
      const serverNameElem = dropdownElement.querySelector(".serverName");
      if (!serverNameElem) {
        console.warn("Could not find .serverName element for", server.name);
        return;
      }

      if (server.active) {
        const playerCount = server.specialized_info?.player_count ?? 0;
        const maxPlayers = server.specialized_info?.max_players ?? 0;
        const isReady = server.specialized_info?.ready ?? false;
        const statusText = isReady ? "ready to join" : "starting";
        serverNameElem.textContent = `${server.name} (${playerCount} / ${maxPlayers}) Status: ${statusText}`;
      } else {
        serverNameElem.textContent = `${server.name} ${RSC.uiText.inactiveSuffix}`;
      }
    }
  }

  class VintageStorySpecialization extends ServerSpecialization {
    updateUI(dropdownElement, server) {
      const serverNameElem = dropdownElement.querySelector(".serverName");
      if (!serverNameElem) {
        console.warn("Could not find .serverName element for", server.name);
        return;
      }

      const playerCount = server.specialized_info?.player_count ?? 0;
      const maxPlayers = server.specialized_info?.max_players ?? 0;
      const calendarPaused = server.specialized_info?.calendar_paused ?? false;
      serverNameElem.textContent = server.active
        ? `${server.name} (${playerCount} / ${maxPlayers}) Calendar Paused: ${calendarPaused}`
        : `${server.name} ${RSC.uiText.inactiveSuffix}`;
    }
  }

  const specializationRegistry = {
    Minecraft: new MinecraftSpecialization(),
    VintageStory: new VintageStorySpecialization(),
  };

  function serverDomKey(serverName) {
    return encodeURIComponent(serverName).replace(/[^a-zA-Z0-9_-]/g, "_");
  }

  function serverClassName(serverName, suffix) {
    return `server-${serverDomKey(serverName)}-${suffix}`;
  }

  function escapeHtml(value) {
    return String(value)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  function ensureServerInfoObj() {
    if (!window.serverInfoObj || !Array.isArray(window.serverInfoObj.servers)) {
      window.serverInfoObj = { servers: [] };
    }
    return window.serverInfoObj;
  }

  function findServerInfo(serverName) {
    return ensureServerInfoObj().servers.find(
      (server) => server.name === serverName,
    );
  }

  function upsertServerInfo(nextServer) {
    const serverInfo = ensureServerInfoObj();
    const existing = findServerInfo(nextServer.name);
    if (existing) {
      Object.assign(existing, nextServer);
      return existing;
    }
    serverInfo.servers.push(nextServer);
    return nextServer;
  }

  function createServerDropdown(serverName, inactive) {
    const titleText = inactive
      ? `${serverName} ${RSC.uiText.inactiveSuffix}`
      : serverName;
    const dropdown = $(
      '<div class="CentralMenuDropdown ' +
        serverClassName(serverName, "dropdown") +
        '"><div class="innerTopBarDropDown"> <p class="serverName">' +
        escapeHtml(titleText) +
        '</p><div class="serverActions"><button type="button" class="serverAction powerAction" title="' +
        RSC.uiText.power +
        '"><svg viewBox="0 0 24 24"><path d="M13 3h-2v10h2V3zm4.83 2.17-1.42 1.42A7 7 0 1 1 7.59 6.59L6.17 5.17A9 9 0 1 0 17.83 5.17z"/></svg></button><button type="button" class="serverAction restartAction" title="' +
        RSC.uiText.restart +
        '"><svg viewBox="0 0 24 24"><path d="M17.65 6.35A7.95 7.95 0 0 0 12 4a8 8 0 1 0 7.75 10h-2.1A6 6 0 1 1 12 6c1.66 0 3.14.69 4.22 1.78L13 11h8V3l-3.35 3.35z"/></svg></button><button type="button" class="serverAction killAction" title="' +
        RSC.uiText.kill +
        '"><svg viewBox="0 0 24 24"><path d="M6 6h12v12H6z"/></svg></button></div><a href="#" class="button dropdownArrow"><svg clip-rule="evenodd" class="bloom" fill-rule="evenodd" stroke-linejoin="round" stroke-miterlimit="2" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">  <path d="m16.843 10.211c.108-.141.157-.3.157-.456 0-.389-.306-.755-.749-.755h-8.501c-.445 0-.75.367-.75.755 0 .157.05.316.159.457 1.203 1.554 3.252 4.199 4.258 5.498.142.184.36.29.592.29.23 0 .449-.107.591-.291zm-7.564.289h5.446l-2.718 3.522z" fill-rule="nonzero"/>  </svg><svg clip-rule="evenodd" fill-rule="evenodd" stroke-linejoin="round" stroke-miterlimit="2" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="m16.843 10.211c.108-.141.157-.3.157-.456 0-.389-.306-.755-.749-.755h-8.501c-.445 0-.75.367-.75.755 0 .157.05.316.159.457 1.203 1.554 3.252 4.199 4.258 5.498.142.184.36.29.592.29.23 0 .449-.107.591-.291zm-7.564.289h5.446l-2.718 3.522z" fill-rule="nonzero"/></svg></a></div><div class="dropdownDrop" style="display: none;"><div class="serverSTDOut ' +
        serverClassName(serverName, "Out") +
        '"></div><div class="serverSTDIn"><div class="STDInRow"><input class="STDInInput" placeholder="' +
        RSC.uiText.stdinPlaceholder +
        '"></input><button type="button" class="STDInSubmit">' +
        RSC.uiText.submit +
        "</button></div></div></div></div>",
    );

    dropdown
      .find(".dropdownArrow")
      .children()
      .css({ transform: "rotate(-180deg)" });
    dropdown.appendTo(RSC.selectors.serverList);
    dropdown.find(".innerTopBarDropDown").click(app.dropdownClick);
    bindServerActions(dropdown, serverName);
    dropdown.find(".dropdownDrop").hide();
    dropdown.toggleClass("inactiveServer", inactive === true);
    bindConsoleInput(dropdown, serverName);
    return dropdown;
  }

  function sendServerAction(type, serverName) {
    app.sendSocketMessage({
      type,
      server_name: serverName,
    });
  }

  function bindServerActions(dropdown, serverName) {
    dropdown.find(".serverAction").click(function (event) {
      event.preventDefault();
      event.stopPropagation();
    });
    dropdown.find(".powerAction").click(function () {
      sendServerAction(RSC.messages.startServer, serverName);
    });
    dropdown.find(".restartAction").click(function () {
      sendServerAction(RSC.messages.restartServer, serverName);
    });
    dropdown.find(".killAction").click(function () {
      sendServerAction(RSC.messages.killServer, serverName);
    });
  }

  function bindConsoleInput(dropdown, serverName) {
    let numBack = 0;
    const input = dropdown.find(".STDInInput");

    function historyValueFromBack(offset) {
      const index = app.state.commandHistory.length - offset;
      return index >= 0 ? app.state.commandHistory[index] : "";
    }

    function handleConsoleInput(inputValue) {
      if (inputValue === "") return;

      const isStartCommand =
        inputValue.trim().toLowerCase() === RSC.commands.start;
      app.sendSocketMessage({
        type: RSC.messages.stdinInput,
        server_name: serverName,
        value: isStartCommand ? RSC.commands.start : inputValue,
      });

      if (isStartCommand && dropdown.hasClass("inactiveServer")) {
        $("." + serverClassName(serverName, "Out")).empty();
        $("." + serverClassName(serverName, "Out")).append(
          $('<p class="STDOutMessage" style="color: #FFFF00;"></p>').text(
            RSC.uiText.startingServer,
          ),
        );
      }

      app.state.commandHistory.push(inputValue);
      if (app.state.commandHistory.length > RSC.commands.historyLimit) {
        app.state.commandHistory.splice(
          0,
          app.state.commandHistory.length - RSC.commands.historyLimit,
        );
      }
      numBack = 0;
    }

    input.keydown(function (e) {
      if (e.which === 13) {
        const inputValue = $(this).val();
        $(this).val("");
        handleConsoleInput(inputValue);
      } else if (e.which === 40) {
        numBack = Math.max(0, numBack - 1);
        $(this).val(numBack > 0 ? historyValueFromBack(numBack) : "");
      } else if (e.which === 38) {
        numBack = Math.min(app.state.commandHistory.length, numBack + 1);
        $(this).val(historyValueFromBack(numBack));
      }
    });

    dropdown.find(".STDInSubmit").click(function (e) {
      if (e.which !== 1) return;
      const inputValue = input.val();
      input.val("");
      handleConsoleInput(inputValue);
    });
  }

  function addDropdownNoDupe(name, inactive) {
    if ($("." + serverClassName(name, "dropdown")).toArray().length === 0) {
      createServerDropdown(name, inactive);
    }
  }

  function processServerLogLines(serverName, logString, isBulk) {
    const outDiv = $("." + serverClassName(serverName, "Out"))[0];
    if (!outDiv) {
      console.warn("Output div not found for server:", serverName);
      return;
    }

    const lines = logString
      .replace(/<br\s*\/?>/gi, "\n")
      .replace(/\r\n/g, "\n")
      .split("\n");
    const lastCount = app.state.lastLogLineCount[serverName] || 0;
    const startIdx = isBulk ? lastCount : 0;
    const shouldScroll = outDiv.scrollTop == outDiv.scrollHeight;

    for (let i = startIdx; i < lines.length; i++) {
      const line = lines[i];
      if (line.trim() !== "") {
        const message = $('<p class="STDOutMessage"></p>').appendTo(outDiv)[0];
        if (message) message.innerHTML = line;
      }
    }

    if (shouldScroll) {
      outDiv.scrollTop = outDiv.scrollHeight;
    }
    if (isBulk) {
      app.state.lastLogLineCount[serverName] = lines.length;
    }
  }

  function setServerTitle(dropdownElement, server) {
    const serverNameElem = dropdownElement.querySelector(".serverName");
    if (!serverNameElem) return;
    serverNameElem.textContent = server.active
      ? server.name
      : `${server.name} ${RSC.uiText.inactiveSuffix}`;
  }

  function syncDropdownState(dropdownElement, server) {
    dropdownElement.classList.toggle("inactiveServer", !server.active);
    dropdownElement.classList.toggle("activeServer", server.active);
    setServerTitle(dropdownElement, server);
  }

  app.mergeServerInfoSnapshot = function (snapshot) {
    window.serverInfoObj = snapshot;
    ensureServerInfoObj().servers.forEach((server) => {
      addDropdownNoDupe(server.name, !server.active);
      processServerLogLines(server.name, server.output || "", true);
    });
    app.updateStats?.();
  };

  app.mergeSpecializationInfoUpdate = function (update) {
    upsertServerInfo({
      name: update.server_name,
      specialized_info: update.info,
      specialization_stats: update.stats ?? null,
      specialization_options: update.specialization_options ?? null,
      active: typeof update.active !== "undefined" ? update.active : false,
      specialization: update.specialization || "",
    });
    app.updateStats?.();
  };

  app.updateServerInfoSpecializations = function () {
    try {
      const serverInfo = window.serverInfoObj;
      if (!serverInfo || !serverInfo.servers) return;
      serverInfo.servers.forEach((server) => {
        addDropdownNoDupe(server.name, !server.active);
        const dropdownElement = document.querySelector(
          `.${serverClassName(server.name, "dropdown")}`,
        );
        if (!dropdownElement) return;
        syncDropdownState(dropdownElement, server);
        const specialization = specializationRegistry[server.specialization];
        if (specialization) {
          specialization.updateUI(dropdownElement, server);
        }
      });
    } catch (e) {
      console.error("Error updating specialization UIs:", e);
    }
  };

  app.processServerLogLines = processServerLogLines;
})(window.RSCApp);
