window.RSCApp = window.RSCApp || {};

(function (app) {
  const RSC = window.RSC_CONSTANTS;

  function oklchToRgb(lightness, chroma, hue) {
    const hRad = hue * (Math.PI / 180);
    const aLab = chroma * Math.cos(hRad);
    const bLab = chroma * Math.sin(hRad);

    const l = lightness + 0.3963377774 * aLab + 0.2158037573 * bLab;
    const m = lightness - 0.1055613458 * aLab - 0.0638541728 * bLab;
    const s = lightness - 0.0894841775 * aLab - 1.291485548 * bLab;

    const l3 = l * l * l;
    const m3 = m * m * m;
    const s3 = s * s * s;

    const rLinear = +4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3;
    const gLinear = -1.2684380046 * l3 + 2.6097574011 * m3 - 0.3413193965 * s3;
    const bLinear = -0.0041960863 * l3 - 0.7034186147 * m3 + 1.707614701 * s3;

    return [rLinear, gLinear, bLinear].map((channel) => {
      const srgb =
        channel <= 0.0031308
          ? 12.92 * channel
          : 1.055 * Math.pow(channel, 1 / 2.4) - 0.055;
      return Math.max(0, Math.min(255, Math.round(srgb * 255)));
    });
  }

  function parseOklch(oklchStr) {
    if (!oklchStr) return [...RSC.webgl.fallbackRgb];

    try {
      const matchResult = oklchStr.match(
        /oklch\s*\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*\)/,
      );
      if (!matchResult) return [...RSC.webgl.fallbackRgb];
      const [, lightness, chroma, hue] = matchResult;
      return oklchToRgb(
        parseFloat(lightness),
        parseFloat(chroma),
        parseFloat(hue),
      );
    } catch (e) {
      console.error("Error parsing OKLCH color:", e);
      return [...RSC.webgl.fallbackRgb];
    }
  }

  function createShader(gl, type, source) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);

    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      console.error("Shader compilation failed:", gl.getShaderInfoLog(shader));
      gl.deleteShader(shader);
      return null;
    }
    return shader;
  }

  function createProgram(gl, vertexShaderSource, fragmentShaderSource) {
    const vertexShader = createShader(gl, gl.VERTEX_SHADER, vertexShaderSource);
    const fragmentShader = createShader(
      gl,
      gl.FRAGMENT_SHADER,
      fragmentShaderSource,
    );
    if (!vertexShader || !fragmentShader) return null;

    const program = gl.createProgram();
    gl.attachShader(program, vertexShader);
    gl.attachShader(program, fragmentShader);
    gl.linkProgram(program);

    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      console.error("Program linking failed:", gl.getProgramInfoLog(program));
      return null;
    }
    return program;
  }

  function drawGradient(gl, canvas, primaryRgb, bgDarkRgb) {
    const vertexShaderSource = `
      attribute vec2 a_position;
      void main() {
        gl_Position = vec4(a_position, 0, 1);
      }
    `;
    const fragmentShaderSource = `
      precision mediump float;
      uniform vec3 u_primaryColor;
      uniform vec3 u_bgColor;
      uniform float u_decayRate;
      float random(vec2 st) {
        return fract(sin(dot(st.xy, vec2(12.9898, 78.233))) * 43758.5453123);
      }
      void main() {
        float x = (gl_FragCoord.x / ${window.innerWidth.toFixed(1)});
        float noise = random(gl_FragCoord.xy) * 0.03 - 0.015;
        float falloff = exp(-u_decayRate * x);
        falloff = clamp(falloff + noise * (1.0 - falloff) * falloff * 3.0, 0.0, 1.0);
        vec3 color = mix(u_bgColor / 255.0, u_primaryColor / 255.0, falloff);
        color = clamp(color + vec3(noise * 0.015), vec3(0.0), vec3(1.0));
        float alpha = 0.85 + (0.15 * falloff);
        gl_FragColor = vec4(color, alpha);
      }
    `;

    const program = createProgram(gl, vertexShaderSource, fragmentShaderSource);
    if (!program) return;
    gl.useProgram(program);

    const positionBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer);
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
      gl.STATIC_DRAW,
    );

    const positionAttributeLocation = gl.getAttribLocation(program, "a_position");
    gl.enableVertexAttribArray(positionAttributeLocation);
    gl.vertexAttribPointer(positionAttributeLocation, 2, gl.FLOAT, false, 0, 0);

    const [primaryR, primaryG, primaryB] = primaryRgb;
    const [bgDarkR, bgDarkG, bgDarkB] = bgDarkRgb;
    gl.uniform3f(
      gl.getUniformLocation(program, "u_primaryColor"),
      primaryR,
      primaryG,
      primaryB,
    );
    gl.uniform3f(
      gl.getUniformLocation(program, "u_bgColor"),
      bgDarkR,
      bgDarkG,
      bgDarkB,
    );
    gl.uniform1f(gl.getUniformLocation(program, "u_decayRate"), RSC.webgl.decayRate);

    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  app.initWebglBackground = function () {
    let canvas = null;
    let gl = null;

    function render() {
      if (
        !canvas ||
        window.innerWidth !== canvas.width ||
        window.innerHeight !== canvas.height
      ) {
        if (gl) {
          const loseCtx = gl.getExtension("WEBGL_lose_context");
          if (loseCtx) loseCtx.loseContext();
          gl = null;
        }
        if (canvas) $(canvas).remove();

        canvas = document.createElement("canvas");
        canvas.className = "overlay";
        canvas.width = window.innerWidth;
        canvas.height = window.innerHeight;
        document.querySelector(RSC.selectors.grad).appendChild(canvas);

        const style = getComputedStyle(document.documentElement);
        const bgDarkRgb = parseOklch(style.getPropertyValue("--bg-dark").trim());
        const primaryRgb = parseOklch(style.getPropertyValue("--primary").trim());

        gl =
          canvas.getContext("webgl") || canvas.getContext("experimental-webgl");
        if (!gl) {
          console.error("WebGL not supported");
          return;
        }

        drawGradient(gl, canvas, primaryRgb, bgDarkRgb);
      }
      requestAnimationFrame(render);
    }

    render();
  };
})(window.RSCApp);
