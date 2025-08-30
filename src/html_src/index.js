window.lastLogLineCount = window.lastLogLineCount || {};

function closeMenu() {
  $("#menu").animate(
    {
      left: "-25%",
    },
    250,
  );
  $("#main-content")
    .animate(
      {
        left: "0%",
      },
      250,
    )
    .animate(
      {
        "background-color": "var(--pageBG)",
      },
      350,
    );
}

function AnimateRotate(selector, start, angle, duration) {
  // caching the object for performance reasons
  var $elem = selector;
  // we use a pseudo object for the animation
  // (starts from `0` to `angle`), you can name it as you want
  $({
    deg: start,
  }).animate(
    {
      deg: angle,
    },
    {
      duration: duration,
      step: function (now) {
        // in the step-callback (that is fired each step of the animation),
        // you can use the `now` paramter which contains the current
        // animation-position (`0` up to `angle`)
        $elem.css({
          transform: "rotate(" + now + "deg)",
        });
      },
    },
  );
}

function dropdownClick(event) {
  event.preventDefault();
  var dropdown = $(this).closest(".CentralMenuDropdown");
  dropdown.toggleClass("open");
  var slider = dropdown.find(".dropdownDrop");
  var arrow = dropdown.find(".dropdownArrow");
  // Toggle the visibility of the ".dropdownDrop" content with a slide animation
  if (slider.hasClass("open")) {
    //close, then hide
    AnimateRotate(arrow.find("svg"), 0, 180, 100);
    slider.slideUp(250, function () {
      slider.hide();
      //$(".CentralMenuDropdown").show();
    });
  } else {
    //show, then open
    //$(".CentralMenuDropdown").hide();
    //dropdown.show();
    AnimateRotate(arrow.find("svg"), 180, 0, 100);
    slider.slideDown(250, function () {
      slider.show();
    });
  }
  slider.toggleClass("open");
}

function get_ws_addr() {
  return (
    document.location.href
      .replace("http", "ws")
      .replace("https", "wss")
      .replace("#", "") + "ws"
  );
}

function hotReloadWhenReady() {
  setInterval(function () {
    try {
      var req = new XMLHttpRequest();
      req.onreadystatechange = function () {
        if (this.status === 200) {
          document.location.reload();
        }
      };
      req.open("GET", document.location.href);
      req.send();
    } catch (e) {}
  }, 1000);
}
window.commands = [];
function addServerDropdown(serverName, inactive) {
  console.log("adding a dropdown");
  let titleText = serverName;
  if (inactive) {
    titleText += " (inactive)";
  }
  var dropdown = $(
    '<div class="CentralMenuDropdown ' +
      serverName +
      'dropdown"><div class="innerTopBarDropDown"> <p class="serverName">' +
      titleText +
      '</p>   <a href="#" class="button dropdownArrow"><svg clip-rule="evenodd" class="bloom" fill-rule="evenodd" stroke-linejoin="round" stroke-miterlimit="2" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">  <path d="m16.843 10.211c.108-.141.157-.3.157-.456 0-.389-.306-.755-.749-.755h-8.501c-.445 0-.75.367-.75.755 0 .157.05.316.159.457 1.203 1.554 3.252 4.199 4.258 5.498.142.184.36.29.592.29.23 0 .449-.107.591-.291zm-7.564.289h5.446l-2.718 3.522z" fill-rule="nonzero"/>  </svg><svg clip-rule="evenodd" fill-rule="evenodd" stroke-linejoin="round" stroke-miterlimit="2" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="m16.843 10.211c.108-.141.157-.3.157-.456 0-.389-.306-.755-.749-.755h-8.501c-.445 0-.75.367-.75.755 0 .157.05.316.159.457 1.203 1.554 3.252 4.199 4.258 5.498.142.184.36.29.592.29.23 0 .449-.107.591-.291zm-7.564.289h5.446l-2.718 3.522z" fill-rule="nonzero"/></svg></a></div><div class="dropdownDrop" style: "display: none;"><div class="serverSTDOut ' +
      serverName +
      'Out"></div><div class="serverSTDIn"><div class="STDInRow"><input class="STDInInput" placeholder="place input for STDIn here..."></input><button type="button" class="STDInSubmit">Submit</button></div></div></div></div>',
  );
  var arrow = dropdown.find(".dropdownArrow").children().css({
    transform: "rotate(-180deg)",
  });
  dropdown.appendTo(".centerMenu.servers");
  dropdown.find(".innerTopBarDropDown").click(dropdownClick);

  var numBack = 0;
  var input = dropdown.find(".STDInInput");

  // Create a helper function to handle the console input consistently
  function handleConsoleInput(inputValue) {
    if (inputValue == "") return;

    // Normalize start command - check if it's a start command (case insensitive and ignoring whitespace)
    var isStartCommand = inputValue.trim().toLowerCase() === "start";

    var obj = {
      type: "stdinInput",
      server_name: serverName,
      value: isStartCommand ? "start" : inputValue,
    };
    socket.send(JSON.stringify(obj));

    // Clear console output if starting an inactive server
    if (isStartCommand && dropdown.hasClass("inactiveServer")) {
      // Clear console output and add starting message
      $("." + serverName + "Out").empty();
      var startingMsg = $(
        '<p class="STDOutMessage" style="color: #FFFF00;">Starting server, please wait...</p>',
      );
      $("." + serverName + "Out").append(startingMsg);
    }

    // Store command in history
    if (commands.length > 25) {
      var commandsTemp = [];
      for (
        var i = Math.max(0, commands.length - 25);
        i < commands.length;
        ++i
      ) {
        commandsTemp.push(commands[i]);
      }
      commands = commandsTemp;
    }
    commands.push(inputValue);
    numBack = 0;
  }

  // Handle keyboard input
  input.keydown(function (e) {
    if (e.which === 13) {
      // Enter key
      var inputValue = $(this).val();
      $(this).val("");
      handleConsoleInput(inputValue);
    } else if (e.which === 40) {
      // Down arrow
      numBack = Math.max(0, numBack - 1);
      $(this).val(numBack > 0 ? commands[commands.length - numBack] : "");
    } else if (e.which === 38) {
      // Up arrow
      numBack = Math.min(commands.length, numBack + 1);
      $(this).val(commands[commands.length - numBack]);
    }
  });

  // Handle submit button click
  dropdown.find(".STDInSubmit").click(function (e) {
    if (e.which === 1) {
      // Left mouse button
      var inputValue = input.val();
      input.val("");
      handleConsoleInput(inputValue);
    }
  });
  dropdown.find(".dropdownDrop").slideUp(1).hide();
  if (inactive == true) {
    dropdown.toggleClass("inactiveServer");
  }
  updateServerInfoMCSpecialization();
}

function addDropdownNoDupe(name, inactive) {
  var q = $("." + name + "dropdown").toArray().length != 0;
  if (q != true) {
    addServerDropdown(name, inactive);
  }
}

function createEvent(type, arguments) {
  var obj = {
    type: type ?? [],
    arguments: arguments ?? [],
  };
  return JSON.stringify(obj);
}
function getClassList(element) {
  return $(element).attr("class").split("/\s+/").join(" ").split(" ");
}
function checkAllServers() {
  var servers = $(".CentralMenuDropdown").toArray();
  //finish this, make sure serverInfo
}
function updateServerInfoMCSpecialization() {
  try {
    const serverInfo = window.serverInfoObj;
    serverInfo.servers.forEach((server) => {
      if (server.specialization === "Minecraft") {
        // Check if the specialization is Minecraft
        const serverElement = $(`.${server.name}dropdown`).find(
          ".serverName",
        )[0];
        if (serverElement) {
          if (server.active) {
            // Expect specialized_info to be an object with player_count, max_players, ready
            let playerCount = 0;
            let maxPlayers = 0;
            let isReady = false;
            if (
              server.specialized_info &&
              typeof server.specialized_info === "object"
            ) {
              playerCount = server.specialized_info.player_count ?? 0;
              maxPlayers = server.specialized_info.max_players ?? 0;
              isReady = server.specialized_info.ready ?? false;
            }
            let statusText = isReady ? "ready to join" : "starting";
            serverElement.textContent = `${server.name} (${playerCount} / ${maxPlayers}) Status: ${statusText}`;
          } else {
            serverElement.textContent = `${server.name} (inactive)`;
          }
        }
      }
    });
  } catch (e) {}
}
function generateSecureSalt(lengthInBytes) {
  lengthInBytes = lengthInBytes / 2;
  const saltArray = new Uint8Array(lengthInBytes);
  crypto.getRandomValues(saltArray);

  // Convert the Uint8Array directly to a hexadecimal string without doubling the length
  let salt = "";
  for (let i = 0; i < saltArray.length; i++) {
    salt += (saltArray[i] < 16 ? "0" : "") + saltArray[i].toString(16);
  }

  return salt;
}
async function hashPasswordWithSalt(password, salt) {
  const encoder = new TextEncoder();
  const data = encoder.encode(password + salt);
  const hashBuffer = await crypto.subtle.digest("SHA-256", data);
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  const hashHex = hashArray
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  return hashHex;
}
$(document).ready(function () {
  var socket;
  socket = new WebSocket(get_ws_addr());
  socket.onerror = function () {
    hotReloadWhenReady();
  };
  // Set interval to update server info every quarter second
  setInterval(updateServerInfoMCSpecialization, 250);

  socket.onclose = function () {
    hotReloadWhenReady();
  };
  socket.addEventListener("open", function () {
    socket.send(createEvent("requestInfo", [true]));
    requestThemesList(); // Request themes when connection is established
  });
  setInterval(function () {
    try {
      socket.send(createEvent("requestInfo", [true]));
      for (var index in window.serverInfoObj.servers) {
        var server = window.serverInfoObj.servers[index];
        var name = server.name;
        var dropdown = $("." + name + "dropdown");
        var title = dropdown.find(".serverName");
        var titleText = title[0].textContent;
        if (
          server.active == false &&
          titleText.endsWith(" (inactive)") == false
        ) {
          title[0].textContent += " (inactive)";
          dropdown.toggleClass("inactiveServer");
        }
        if (dropdown.hasClass("inactiveServer") && server.active) {
          title[0].textContent = title[0].textContent
            .split(" (inactive)")
            .join("");
          dropdown.toggleClass("inactiveServer");
        }
      }
      checkAllServers();
    } catch (e) {}
  }, 200);
  window.config = {
    state: "NotInit",
  };

  // Theme functions
  function applyTheme(themeName, cssContent) {
    // Remove any existing theme style tag
    const existingThemeStyle = document.getElementById("current-theme");
    if (existingThemeStyle) {
      existingThemeStyle.remove();
    }

    // Create and add new style tag with theme CSS
    const themeStyle = document.createElement("style");
    themeStyle.id = "current-theme";
    themeStyle.textContent = cssContent;
    document.head.appendChild(themeStyle);

    // Save to localStorage
    localStorage.setItem("selectedTheme", themeName);
    localStorage.setItem("themeCSS", cssContent);

    // Update UI if there's a theme selector
    const themeSelector = document.getElementById("theme-selector");
    if (themeSelector) {
      themeSelector.value = themeName;

      // Remove loading indicator if it exists
      const loadingIndicator = document.querySelector(".theme-loading");
      if (loadingIndicator) {
        loadingIndicator.style.display = "none";
      }
    }

    // Theme applied

    // Add a subtle transition effect when changing themes
    document.body.style.transition =
      "background-color 0.3s ease, color 0.3s ease";
    setTimeout(() => {
      document.body.style.transition = "";
    }, 300);
  }

  function loadThemeFromStorage() {
    const themeName = localStorage.getItem("selectedTheme");
    const themeCSS = localStorage.getItem("themeCSS");

    if (themeName && themeCSS) {
      // Loading theme from localStorage
      applyTheme(themeName, themeCSS);
      return themeName;
    }
    // No theme found in localStorage
    return null;
  }

  function requestThemesList() {
    if (socket && socket.readyState === WebSocket.OPEN) {
      // Requesting themes list from server
      const msg = {
        type: "getThemesList",
      };
      socket.send(JSON.stringify(msg));
      return true;
    }
    return false;
  }

  function requestThemeCSS(themeName) {
    if (socket && socket.readyState === WebSocket.OPEN) {
      // Requesting theme CSS
      const msg = {
        type: "getThemeCSS",
        theme_name: themeName,
      };
      socket.send(JSON.stringify(msg));

      // Add or update loading indicator next to theme selector
      let loadingIndicator = document.querySelector(".theme-loading");
      if (!loadingIndicator) {
        const themeContainer = document.querySelector(".theme-container");
        if (themeContainer) {
          loadingIndicator = document.createElement("span");
          loadingIndicator.className = "theme-loading";
          themeContainer.appendChild(loadingIndicator);
        }
      }

      if (loadingIndicator) {
        loadingIndicator.style.display = "inline";
        loadingIndicator.textContent = "Loading...";
      }

      return true;
    }
    return false;
  }

  var justStarted = true;
  var editor; // Monaco editor instance

  // Send requestConfig at startup
  function sendRequestConfig() {
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: "requestConfig" }));
    } else {
      setTimeout(sendRequestConfig, 100);
    }
  }

  // Initialize Monaco Editor
  function initMonacoEditor() {
    if (typeof monaco !== "undefined") {
      // Define custom theme to match the app's colors
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

      // Create editor
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

      // Sync editor with textarea
      editor.onDidChangeModelContent(function () {
        $(".editorText").val(editor.getValue());
      });
    }
  }

  // Wait for Monaco to be loaded
  require(["vs/editor/editor.main"], function () {
    initMonacoEditor();
  });

  socket.onmessage = function (message) {
    var obj = JSON.parse(message.data);
    switch (obj.type) {
      case "ConfigInfo":
        // Always update config editor with received config
        window.config = obj.config;
        const jsonConfig = JSON.stringify(obj.config, undefined, 4);
        $(".editorText").val(jsonConfig);
        if (editor) {
          editor.setValue(jsonConfig);
        }
        justStarted = false;
        break;
      case "ServerInfo":
        for (index in obj.servers) {
          var server = obj.servers[index];
          let serverName = server.name;
          addDropdownNoDupe(serverName, !server.active);
          var outDiv = $("." + serverName + "Out");
          var lines = server.output.split("\r\n");
          var lastCount = window.lastLogLineCount[serverName] || 0;
          // Only append new lines
          for (let i = lastCount; i < lines.length; i++) {
            var line = lines[i];
            if (line.trim() !== "") {
              var p = $('<p class="STDOutMessage"></p>').appendTo(outDiv)[0];
              p.innerHTML = line;
            }
          }
          window.lastLogLineCount[serverName] = lines.length;
        }
        window.serverInfoObj = obj;
        break;
      case "ServerOutput":
        var str = obj.output;
        // Split on <br> (or <br/>), so each log line is its own <p>
        var lines = str.split(/<br\s*\/?>/i);
        var outDiv = $("." + obj.server_name + "Out")[0];
        if (!outDiv) {
          console.warn("Output div not found for server:", obj.server_name);
          break;
        }
        var shouldScroll = outDiv.scrollTop == outDiv.scrollHeight;
        lines.forEach(function (line) {
          if (line.trim() !== "") {
            var p = $('<p class="STDOutMessage"></p>').appendTo(outDiv)[0];
            p.innerHTML = line;
          }
        });
        if (shouldScroll) {
          outDiv.scrollTop = outDiv.scrollHeight;
        }
        break;
      case "themesList":
        // Received themes list from server
        // Handle received list of themes
        const themes = obj.themes;
        if (themes && Array.isArray(themes) && themes.length > 0) {
          // Check if we already have a theme selector, create one if not
          let themeSelector = document.getElementById("theme-selector");
          if (!themeSelector) {
            // Create theme selector dropdown
            themeSelector = document.createElement("select");
            themeSelector.id = "theme-selector";

            // Add to the UI - placing it in the inner top bar
            const innerTopBar = document.querySelector(".innerTopBar");
            if (innerTopBar) {
              const themeContainer = document.createElement("div");
              themeContainer.className = "theme-container";
              themeContainer.innerHTML =
                '<label for="theme-selector">Theme</label>';
              themeContainer.appendChild(themeSelector);

              // Add loading indicator
              const loadingIndicator = document.createElement("span");
              loadingIndicator.className = "theme-loading";
              loadingIndicator.textContent = "Loading...";
              loadingIndicator.style.display = "none";
              themeContainer.appendChild(loadingIndicator);

              innerTopBar.appendChild(themeContainer);
            }

            // Add change event listener
            themeSelector.addEventListener("change", function () {
              const selectedTheme = this.value;
              requestThemeCSS(selectedTheme);
            });
          } else {
            // Clear existing options
            themeSelector.innerHTML = "";
          }

          // Populate options
          themes.forEach((theme) => {
            const option = document.createElement("option");
            option.value = theme;
            option.textContent = theme;
            themeSelector.appendChild(option);
          });

          // Get current theme from localStorage or use first theme
          const currentTheme =
            localStorage.getItem("selectedTheme") || themes[0];
          themeSelector.value = currentTheme;

          // Request CSS for the selected theme
          requestThemeCSS(currentTheme);
        }
        break;
      case "themeCSS":
        // Received theme CSS
        // Apply received theme CSS
        applyTheme(obj.theme_name, obj.css);
        break;
    }
  };

  // Initialize theme system
  // Initializing theme system

  // First try to load from localStorage
  const loadedTheme = loadThemeFromStorage();

  if (loadedTheme) {
    // Theme loaded from localStorage
  } else {
    // No theme in localStorage, will request from server
  }

  // Server will send theme list after connection

  // Request config at startup (only once)
  sendRequestConfig();

  // if (typeof InstallTrigger !== 'undefined') { // Check if Firefox
  // 	document.getElementById('scrollable-div').classList.add('scrollbar'); // broke firefox
  // }
  var saveTimeout = undefined;
  $(".configSave").click(function (e) {
    if (e.which !== 1) return;
    if (saveTimeout !== undefined) clearTimeout(saveTimeout);

    try {
      // Get content from Monaco editor if available, otherwise from textarea
      const jsonContent = editor ? editor.getValue() : $(".editorText").val();
      var newConfig = JSON.parse(jsonContent);
      var obj = {
        type: "configChange",
        updatedConfig: newConfig,
      };
      socket.send(JSON.stringify(obj));

      // Format the JSON and update the editor
      const formattedJson = JSON.stringify(newConfig, null, 4);
      if (editor) {
        editor.setValue(formattedJson);
      } else {
        $(".editorText").val(formattedJson);
      }

      // Visual feedback for save
      $(this).find(".bloom").css("opacity", "1");
      setTimeout(() => {
        $(this).find(".bloom").css("opacity", "0");
      }, 800);
    } catch (error) {
      alert("Invalid JSON: " + error.message);
    }
  });
  window.socket = socket;

  // Add some CSS for theme selector
  const themeStyles = document.createElement("style");
  themeStyles.textContent = `
    .theme-container {
      display: flex;
      align-items: center;
      position: absolute;
      right: 80px;
      top: 50%;
      transform: translateY(-50%);
      z-index: 10;
    }
    .theme-container label {
      margin-right: 10px;
      color: var(--text, white);
      font-family: 'Roboto', sans-serif;
      font-weight: 500;
      text-shadow: 0 0 3px rgba(0, 0, 0, 0.4);
    }
    #theme-selector {
      background-color: var(--bg-dark, #222);
      color: var(--text, white);
      border: 1px solid var(--border, #555);
      border-radius: 5px;
      padding: 6px 12px;
      font-size: 14px;
      font-family: 'Roboto', sans-serif;
      cursor: pointer;
      outline: none;
      transition: all 0.2s ease;
      box-shadow: 0 0 5px rgba(0, 0, 0, 0.3);
      min-width: 120px;
    }
    #theme-selector:hover {
      border-color: var(--primary, #777);
      box-shadow: 0 0 8px var(--primary, #777);
    }
    #theme-selector:focus {
      border-color: var(--primary, #777);
      box-shadow: 0 0 12px var(--primary, #777);
    }
    #theme-selector option {
      background: var(--bg, #2a2a2a);
      color: var(--text, white);
      padding: 8px;
    }
    .theme-loading {
      margin-left: 8px;
      color: var(--primary, #777);
      font-size: 12px;
      font-family: 'Roboto', sans-serif;
      animation: pulse 1.5s infinite ease-in-out;
    }
    @keyframes pulse {
      0% { opacity: 0.5; }
      50% { opacity: 1; }
      100% { opacity: 0.5; }
    }
  `;
  document.head.appendChild(themeStyles);

  $("#menu").animate(
    {
      left: "-25%",
    },
    1,
  );
  $(".menuBTN1").click(function (e) {
    if (e.which != 1) return;
    socket.send(createEvent("terminateServers"));
  });
  var classMap = {
    Servers: ".servers",
    Configuration: ".config",
    Stats: ".stat",
  };
  $("#menu ul li a").click(function (e) {
    $(".active").toggleClass("active");
    $(e.target.parentElement).toggleClass("active");
    $(".page").hide();
    $(".grad").show();
    $(classMap[e.target.innerHTML]).show();
  });
  $($("#menu ul li")[0]).toggleClass("active");
  $(".page").hide();
  $(".servers").show();
  var menuOpen = false;
  $("#menu-icon").click(function () {
    if (menuOpen) {
      closeMenu();
      $(".overlay").fadeTo(350, 0.0, "linear", function () {
        $(".grad").hide();
      });
    } else {
      $("#menu").animate(
        {
          left: "0%",
        },
        250,
      );
      $("#main-content").animate(
        {
          left: "25%",
        },
        250,
      );
      setTimeout(function () {
        $(".grad").show();
        $(".overlay").fadeTo(350, 1.0, "linear");
      }, 100);
    }
    setTimeout(function () {
      menuOpen = !menuOpen;
    }, 250);
  });
  $("#main-content").hover(function () {
    if (menuOpen) {
      closeMenu();
      $(".overlay").fadeTo(350, 0.0, "linear", function () {
        $(".grad").hide();
      });
    }
    menuOpen = false;
  });
  // canvas will be created and managed globally for WebGL overlay below
  var dropdownSelector = $(".centerMenu.CentralMenuDropdown");
  dropdownSelector.click();

  // Convert OKLCH color to RGB - with fixed variable names to avoid redeclaration
  function oklchToRgb(lightness, chroma, hue) {
    // Convert OKLCH to OKLab
    const L = lightness;
    const C = chroma;
    const h_rad = hue * (Math.PI / 180);
    const a_lab = C * Math.cos(h_rad);
    const b_lab = C * Math.sin(h_rad);

    // Convert OKLab to linear RGB
    const l_ = L + 0.3963377774 * a_lab + 0.2158037573 * b_lab;
    const m_ = L - 0.1055613458 * a_lab - 0.0638541728 * b_lab;
    const s_ = L - 0.0894841775 * a_lab - 1.291485548 * b_lab;

    const l_cubed = l_ * l_ * l_;
    const m_cubed = m_ * m_ * m_;
    const s_cubed = s_ * s_ * s_;

    // Convert to linear RGB
    const r_linear =
      +4.0767416621 * l_cubed - 3.3077115913 * m_cubed + 0.2309699292 * s_cubed;
    const g_linear =
      -1.2684380046 * l_cubed + 2.6097574011 * m_cubed - 0.3413193965 * s_cubed;
    const b_linear =
      -0.0041960863 * l_cubed - 0.7034186147 * m_cubed + 1.707614701 * s_cubed;

    // Convert to sRGB
    const r_srgb =
      r_linear <= 0.0031308
        ? 12.92 * r_linear
        : 1.055 * Math.pow(r_linear, 1 / 2.4) - 0.055;
    const g_srgb =
      g_linear <= 0.0031308
        ? 12.92 * g_linear
        : 1.055 * Math.pow(g_linear, 1 / 2.4) - 0.055;
    const b_srgb =
      b_linear <= 0.0031308
        ? 12.92 * b_linear
        : 1.055 * Math.pow(b_linear, 1 / 2.4) - 0.055;

    // Clamp values to valid RGB range and convert to 0-255
    const r_255 = Math.max(0, Math.min(255, Math.round(r_srgb * 255)));
    const g_255 = Math.max(0, Math.min(255, Math.round(g_srgb * 255)));
    const b_255 = Math.max(0, Math.min(255, Math.round(b_srgb * 255)));

    return [r_255, g_255, b_255];
  }

  // Parse OKLCH string like "oklch(0.1 0.01 256)" and return RGB values
  function parseOklch(oklchStr) {
    // If the string is empty or undefined, return default values
    if (!oklchStr) return [30, 30, 50];

    try {
      // Use regex to extract the three OKLCH values
      const regex = /oklch\s*\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*\)/;
      const matchResult = oklchStr.match(regex);

      if (matchResult) {
        const l = parseFloat(matchResult[1]);
        const c = parseFloat(matchResult[2]);
        const h = parseFloat(matchResult[3]);
        return oklchToRgb(l, c, h);
      }
      // Fallback values if regex doesn't match
      return [30, 30, 50]; // Dark blue-gray
    } catch (e) {
      console.error("Error parsing OKLCH color:", e);
      return [30, 30, 50]; // Dark blue-gray fallback
    }
  }

  // Keep track of the previous WebGL context for cleanup
  let prevGL = null;
  // Global canvas and gl references
  let canvas = null;
  let gl = null;

  function handleCanvas() {
    if (
      !canvas ||
      window.innerWidth !== canvas.width ||
      window.innerHeight !== canvas.height
    ) {
      // Clean up previous WebGL context if it exists
      if (gl) {
        const loseCtx = gl.getExtension("WEBGL_lose_context");
        if (loseCtx) {
          loseCtx.loseContext();
        }
        gl = null;
      }
      if (canvas) $(canvas).remove();

      canvas = document.createElement("canvas");
      canvas.className = "overlay";
      canvas.width = window.innerWidth;
      canvas.height = window.innerHeight;
      document.querySelector(".grad").appendChild(canvas);

      // Get theme colors from CSS variables
      const style = getComputedStyle(document.documentElement);
      const bgDarkOklch = style.getPropertyValue("--bg-dark").trim();
      const primaryOklch = style.getPropertyValue("--primary").trim();

      // Convert OKLCH colors to RGB
      const bgDarkRgb = parseOklch(bgDarkOklch);
      const primaryRgb = parseOklch(primaryOklch);

      // Initialize WebGL context
      gl =
        canvas.getContext("webgl") || canvas.getContext("experimental-webgl");
      if (!gl) {
        console.error("WebGL not supported");
        return;
      }

      // Create vertex shader - Simple fullscreen quad
      const vertexShaderSource = `
        attribute vec2 a_position;
        void main() {
          gl_Position = vec4(a_position, 0, 1);
        }
      `;

      // Create fragment shader - Exponential gradient with film grain noise
      const fragmentShaderSource = `
        precision mediump float;

        uniform vec3 u_primaryColor;
        uniform vec3 u_bgColor;
        uniform float u_decayRate;

        // Simple pseudo-random function
        float random(vec2 st) {
          return fract(sin(dot(st.xy, vec2(12.9898, 78.233))) * 43758.5453123);
        }

        void main() {
          // X position from -1 to 1, normalize to 0 to 1
          float x = (gl_FragCoord.x / ${window.innerWidth.toFixed(1)});

          // Get pixel coordinates for noise generation
          vec2 pixelPos = gl_FragCoord.xy;

          // Generate subtle noise (film grain effect)
          float noise = random(pixelPos) * 0.03 - 0.015; // ±1.5% noise

          // Exponential falloff function
          float falloff = exp(-u_decayRate * x);

          // Apply noise to the falloff (more noticeable in gradient areas)
          falloff = clamp(falloff + noise * (1.0 - falloff) * falloff * 3.0, 0.0, 1.0);

          // Mix the colors based on the falloff
          vec3 color = mix(u_bgColor / 255.0, u_primaryColor / 255.0, falloff);

          // Add subtle noise to each color channel to break up banding
          color.r += noise * 0.015;
          color.g += noise * 0.015;
          color.b += noise * 0.015;

          // Ensure colors stay in valid range
          color = clamp(color, vec3(0.0), vec3(1.0));

          // Alpha is 0.85 (bg) to 1.0 (highlight)
          float alpha = 0.85 + (0.15 * falloff);

          gl_FragColor = vec4(color, alpha);
        }
      `;

      // Create shader program
      function createShader(gl, type, source) {
        const shader = gl.createShader(type);
        gl.shaderSource(shader, source);
        gl.compileShader(shader);

        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
          console.error(
            "Shader compilation failed:",
            gl.getShaderInfoLog(shader),
          );
          gl.deleteShader(shader);
          return null;
        }
        return shader;
      }

      // Create and link program
      const vertexShader = createShader(
        gl,
        gl.VERTEX_SHADER,
        vertexShaderSource,
      );
      const fragmentShader = createShader(
        gl,
        gl.FRAGMENT_SHADER,
        fragmentShaderSource,
      );

      const program = gl.createProgram();
      gl.attachShader(program, vertexShader);
      gl.attachShader(program, fragmentShader);
      gl.linkProgram(program);

      if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
        console.error("Program linking failed:", gl.getProgramInfoLog(program));
        return;
      }

      gl.useProgram(program);

      // Set up geometry - just a simple full-screen quad
      const positionBuffer = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer);
      const positions = [-1, -1, 1, -1, -1, 1, 1, 1];
      gl.bufferData(
        gl.ARRAY_BUFFER,
        new Float32Array(positions),
        gl.STATIC_DRAW,
      );

      // Set up attributes
      const positionAttributeLocation = gl.getAttribLocation(
        program,
        "a_position",
      );
      gl.enableVertexAttribArray(positionAttributeLocation);
      gl.vertexAttribPointer(
        positionAttributeLocation,
        2,
        gl.FLOAT,
        false,
        0,
        0,
      );

      // Set up uniforms
      const primaryColorLocation = gl.getUniformLocation(
        program,
        "u_primaryColor",
      );
      const bgColorLocation = gl.getUniformLocation(program, "u_bgColor");
      const decayRateLocation = gl.getUniformLocation(program, "u_decayRate");

      // Set uniform values
      gl.uniform3f(
        primaryColorLocation,
        primaryRgb[0],
        primaryRgb[1],
        primaryRgb[2],
      );
      gl.uniform3f(bgColorLocation, bgDarkRgb[0], bgDarkRgb[1], bgDarkRgb[2]);
      gl.uniform1f(decayRateLocation, 50.0); // Adjust for desired falloff speed

      // Set viewport and clear
      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);

      // Draw the quad
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    }
    requestAnimationFrame(handleCanvas);
  }
  handleCanvas();
});
