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
      'Out"></div><div class="serverSTDIn"><input class="STDInInput" placeholder="place input for STDIn here..."></input><a href="#" class="STDInSubmit">Submit</a></div></div></div>',
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
            const specializedInfo = server.specializedInfo["Minecraft"];
            const [playerCount, maxPlayers, isReady] = specializedInfo;
            const statusText = isReady ? "Ready To Join" : "Starting up";
            serverElement.textContent = `${server.name} (${playerCount}/${maxPlayers}) (${statusText})`;
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
  var justStarted = true;
  socket.onmessage = function (message) {
    var obj = JSON.parse(message.data);
    switch (obj.type) {
      case "ServerInfo":
        for (index in obj.servers) {
          var server = obj.servers[index];
          let serverName = server.name;
          addDropdownNoDupe(serverName, !server.active);
          if (justStarted) {
            window.config = obj.config;
            $(".editorText").val(JSON.stringify(obj.config, undefined, 4));
            justStarted = false;
            var lines = server.output.split("\r\n");
            for (linePos in lines) {
              var line = lines[linePos];
              var p = $('<p class="STDOutMessage"></p>').appendTo(
                "." + serverName + "Out",
              )[0];
              p.innerHTML = line;
            }
          }
          if (JSON.stringify(window.config) != JSON.stringify(obj.config)) {
            //the config is out of sync, copy it to the config textarea
            $(".editorText").val(JSON.stringify(obj.config, undefined, 4));
            window.config = obj.config;
          }
        }
        window.serverInfoObj = obj;
        break;
      case "ServerOutput":
        var str = obj.output;
        var lines = str.split("\r\n");
        for (var i in lines) {
          var line = lines[i];
          if (line != "") {
            var outDiv = $("." + obj.server_name + "Out")[0];
            var shouldScroll = outDiv.scrollTop == outDiv.scrollHeight;
            var p = $(' <p class="STDOutMessage"></p>').appendTo(outDiv)[0];
            p.innerHTML = line;
            if (true) {
              outDiv.scrollTop = outDiv.scrollHeight;
            }
          }
        }
        break;
    }
  };
  // if (typeof InstallTrigger !== 'undefined') { // Check if Firefox
  // 	document.getElementById('scrollable-div').classList.add('scrollbar'); // broke firefox
  // }
  var saveTimeout = undefined;
  $(".configSave").click(function (e) {
    if (e.which !== 1) return;
    if (saveTimeout !== undefined) clearTimeout(saveTimeout);
    var newConfig = JSON.parse($(".editorText").val());
    var obj = {
      type: "configChange",
      updatedConfig: newConfig,
    };
    socket.send(JSON.stringify(obj));
  });
  window.socket = socket;
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
  var canvas = $(
    '<canvas class="overlay" width=' + 800 + " height=" + 2 + "></canvas>",
  ).appendTo($(".grad"))[0];
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

  function handleCanvas() {
    if (
      window.innerWidth != canvas.width ||
      window.innerHeight != canvas.height
    ) {
      $(canvas).remove();
      canvas = $(
        ' <canvas class="overlay" width=' +
          window.innerWidth +
          " height=" +
          window.innerHeight +
          "></canvas>",
      ).appendTo($(".grad"))[0];

      // Get theme colors from CSS variables
      const style = getComputedStyle(document.documentElement);
      const bgDarkOklch = style.getPropertyValue("--bg-dark").trim();
      const primaryOklch = style.getPropertyValue("--primary").trim();

      // Convert OKLCH colors to RGB
      const bgDarkRgb = parseOklch(bgDarkOklch);
      const primaryRgb = parseOklch(primaryOklch);

      // Initialize WebGL context
      const gl =
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
