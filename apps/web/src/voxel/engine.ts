import { AIR, BLOCK_BY_ID, BLOCKS, isOpaque } from "./blocks";
import { cameraBasis, lookAt, multiply, normalize, perspective, type CameraBasis, type Vec3 } from "./math";
import { buildMesh, VERTEX_FLOATS, type MeshData } from "./mesher";
import { castRay, type RayHit } from "./raycast";
import { TILE_NAMES, TILE_SIZE, tileLayer, tilePixels } from "./tiles";
import { generateIsland, World, WORLD_FOCUS } from "./world";

export type ToolMode = "break" | "place";

export interface DebugInfo {
  readonly fps: number;
  readonly faces: number;
  readonly blocks: number;
  /** 准星指着的方块。 / The block under the pointer. */
  readonly target: { readonly position: Readonly<Vec3>; readonly id: number } | null;
}

export interface VoxelEngine {
  setSelected(blockId: number): void;
  setMode(mode: ToolMode): void;
  setNight(night: boolean): void;
  reset(): void;
  dispose(): void;
}

export interface EngineOptions {
  /** 每秒调用四次，用来刷新调试文字。 / Called four times a second to refresh the debug text. */
  readonly onDebug?: (info: DebugInfo) => void;
  /** 用户要求减少动画时，不自动旋转，竹子也不摇。 / With reduced motion the island does not spin and bamboo does not sway. */
  readonly reducedMotion?: boolean;
}

const VERTEX_SHADER = `#version 300 es
layout(location = 0) in vec3 aPosition;
layout(location = 1) in vec3 aTexture;
layout(location = 2) in vec4 aLight;

uniform mat4 uViewProjection;
uniform float uTime;
uniform float uWaterLayer;

out vec3 vTexture;
out vec2 vLight;

void main() {
  vec3 position = aPosition;
  float sway = aLight.z;
  float phase = aLight.w;
  position.x += sin(uTime * 1.5 + phase) * sway;
  position.z += cos(uTime * 1.1 + phase * 1.7) * sway * 0.7;

  vec3 coordinate = aTexture;
  if (coordinate.z == uWaterLayer) {
    coordinate.y += uTime * 0.18;
    coordinate.x += sin(uTime * 0.4) * 0.06;
  }

  vTexture = coordinate;
  vLight = aLight.xy;
  gl_Position = uViewProjection * vec4(position, 1.0);
}`;

const FRAGMENT_SHADER = `#version 300 es
precision highp float;
precision highp sampler2DArray;

uniform sampler2DArray uAtlas;
uniform vec3 uSkyLight;
uniform vec3 uBlockLight;
uniform float uTranslucent;

in vec3 vTexture;
in vec2 vLight;
out vec4 outColor;

void main() {
  vec4 texel = texture(uAtlas, vTexture);
  if (texel.a < 0.5) {
    discard;
  }
  vec3 light = max(uSkyLight * vLight.x, uBlockLight * vLight.y);
  outColor = vec4(texel.rgb * light, mix(1.0, texel.a, uTranslucent));
}`;

const LINE_VERTEX_SHADER = `#version 300 es
layout(location = 0) in vec3 aPosition;
uniform mat4 uViewProjection;
uniform vec3 uOrigin;
void main() {
  gl_Position = uViewProjection * vec4(aPosition + uOrigin, 1.0);
}`;

const LINE_FRAGMENT_SHADER = `#version 300 es
precision mediump float;
uniform vec4 uColor;
out vec4 outColor;
void main() {
  outColor = uColor;
}`;

const FIELD_OF_VIEW = (30 * Math.PI) / 180;
/** 画面要容下的范围：竖直方向和水平方向各一个半径。 / The extent the view must contain: one radius vertically and one horizontally. */
const FIT_VERTICAL = 15.5;
const FIT_HORIZONTAL = 20;
const MIN_PITCH = 0.2;
const MAX_PITCH = 1.15;
const AUTO_SPIN = 0.12;
const IDLE_BEFORE_SPIN = 2.5;
const DRAG_THRESHOLD = 5;

const DAY_LIGHT: Vec3 = [1, 1, 1];
const NIGHT_LIGHT: Vec3 = [0.3, 0.36, 0.58];
const LAMP_LIGHT: Vec3 = [1, 0.86, 0.6];

function compile(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type);
  if (!shader) {
    throw new Error("could not create a shader");
  }
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    throw new Error(`shader failed to compile: ${gl.getShaderInfoLog(shader) ?? ""}`);
  }
  return shader;
}

function link(gl: WebGL2RenderingContext, vertexSource: string, fragmentSource: string): WebGLProgram {
  const program = gl.createProgram();
  if (!program) {
    throw new Error("could not create a program");
  }
  gl.attachShader(program, compile(gl, gl.VERTEX_SHADER, vertexSource));
  gl.attachShader(program, compile(gl, gl.FRAGMENT_SHADER, fragmentSource));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(`program failed to link: ${gl.getProgramInfoLog(program) ?? ""}`);
  }
  return program;
}

/** 一份上传到显卡的网格。 / One mesh uploaded to the GPU. */
class GpuMesh {
  private readonly array: WebGLVertexArrayObject;
  private readonly vertexBuffer: WebGLBuffer;
  private indexCount = 0;

  constructor(private readonly gl: WebGL2RenderingContext) {
    const array = gl.createVertexArray();
    const vertexBuffer = gl.createBuffer();
    const indexBuffer = gl.createBuffer();
    if (!array || !vertexBuffer || !indexBuffer) {
      throw new Error("could not create mesh buffers");
    }
    this.array = array;
    this.vertexBuffer = vertexBuffer;

    const stride = VERTEX_FLOATS * 4;
    gl.bindVertexArray(array);
    gl.bindBuffer(gl.ARRAY_BUFFER, vertexBuffer);
    // 索引缓冲的绑定记在顶点数组对象里，之后不用再单独存它。
    // The index buffer binding is stored in the vertex array object, so it needs no field.
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuffer);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, stride, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 3, gl.FLOAT, false, stride, 12);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribPointer(2, 4, gl.FLOAT, false, stride, 24);
    gl.bindVertexArray(null);
  }

  upload(data: MeshData) {
    const { gl } = this;
    gl.bindVertexArray(this.array);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vertexBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, data.vertices, gl.DYNAMIC_DRAW);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, data.indices, gl.DYNAMIC_DRAW);
    gl.bindVertexArray(null);
    this.indexCount = data.indices.length;
  }

  draw() {
    if (this.indexCount === 0) {
      return;
    }
    const { gl } = this;
    gl.bindVertexArray(this.array);
    gl.drawElements(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_INT, 0);
  }
}

/** 一个单位立方体的 12 条棱，画选中框用。 / The 12 edges of a unit cube, for the selection outline. */
function cubeEdges(): Float32Array {
  const low = -0.004;
  const high = 1.004;
  const corners: Vec3[] = [];
  for (const y of [low, high]) {
    corners.push([low, y, low], [high, y, low], [high, y, high], [low, y, high]);
  }
  const lines: number[] = [];
  for (let index = 0; index < 4; index += 1) {
    const next = (index + 1) % 4;
    lines.push(...corners[index], ...corners[next]);
    lines.push(...corners[index + 4], ...corners[next + 4]);
    lines.push(...corners[index], ...corners[index + 4]);
  }
  return new Float32Array(lines);
}

class Engine implements VoxelEngine {
  private readonly gl: WebGL2RenderingContext;
  private readonly world = new World();
  private readonly program: WebGLProgram;
  private readonly lineProgram: WebGLProgram;
  private readonly uniform: Record<string, WebGLUniformLocation | null>;
  private readonly lineUniform: Record<string, WebGLUniformLocation | null>;
  private readonly solid: GpuMesh;
  private readonly water: GpuMesh;
  private readonly outline: WebGLVertexArrayObject;

  private yaw = 0.72;
  private pitch = 0.5;
  private eye: Vec3 = [0, 0, 0];
  private basis: CameraBasis = { forward: [0, 0, -1], right: [1, 0, 0], up: [0, 1, 0] };
  private centerShift: [number, number] = [0, 0];
  private aspect = 1;

  private selected: number = BLOCKS.grass.id;
  private mode: ToolMode = "break";
  private night = false;
  private nightBlend = 0;
  private hover: RayHit | null = null;

  private pointerId: number | null = null;
  private pointerStart: [number, number] = [0, 0];
  private pointerLast: [number, number] = [0, 0];
  private dragging = false;
  private idleFor = IDLE_BEFORE_SPIN;

  private frame = 0;
  private lastTime = 0;
  private clock = 0;
  private faces = 0;
  private blocks = 0;
  private fps = 60;
  private sinceDebug = 0;
  private visible = true;
  private disposed = false;

  private readonly resizeObserver: ResizeObserver;
  private readonly visibilityObserver: IntersectionObserver;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly options: EngineOptions,
  ) {
    const gl = canvas.getContext("webgl2", { antialias: false, alpha: true, premultipliedAlpha: true });
    if (!gl) {
      throw new Error("WebGL2 is not available");
    }
    this.gl = gl;

    this.program = link(gl, VERTEX_SHADER, FRAGMENT_SHADER);
    this.lineProgram = link(gl, LINE_VERTEX_SHADER, LINE_FRAGMENT_SHADER);
    this.uniform = {
      viewProjection: gl.getUniformLocation(this.program, "uViewProjection"),
      time: gl.getUniformLocation(this.program, "uTime"),
      waterLayer: gl.getUniformLocation(this.program, "uWaterLayer"),
      atlas: gl.getUniformLocation(this.program, "uAtlas"),
      skyLight: gl.getUniformLocation(this.program, "uSkyLight"),
      blockLight: gl.getUniformLocation(this.program, "uBlockLight"),
      translucent: gl.getUniformLocation(this.program, "uTranslucent"),
    };
    this.lineUniform = {
      viewProjection: gl.getUniformLocation(this.lineProgram, "uViewProjection"),
      origin: gl.getUniformLocation(this.lineProgram, "uOrigin"),
      color: gl.getUniformLocation(this.lineProgram, "uColor"),
    };

    this.createAtlas();
    this.solid = new GpuMesh(gl);
    this.water = new GpuMesh(gl);
    this.outline = this.createOutline();

    generateIsland(this.world);
    this.remesh();

    canvas.addEventListener("pointerdown", this.onPointerDown);
    canvas.addEventListener("pointermove", this.onPointerMove);
    canvas.addEventListener("pointerup", this.onPointerUp);
    canvas.addEventListener("pointercancel", this.onPointerCancel);
    canvas.addEventListener("pointerleave", this.onPointerLeave);
    canvas.addEventListener("contextmenu", this.onContextMenu);

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(canvas);
    this.visibilityObserver = new IntersectionObserver(([entry]) => {
      this.visible = entry.isIntersecting;
      if (this.visible) {
        this.schedule();
      }
    });
    this.visibilityObserver.observe(canvas);

    this.resize();
    this.schedule();
  }

  // ---- 对外接口 / Public interface ----

  setSelected(blockId: number) {
    this.selected = blockId;
  }

  setMode(mode: ToolMode) {
    this.mode = mode;
  }

  setNight(night: boolean) {
    this.night = night;
  }

  reset() {
    generateIsland(this.world);
    this.world.lampsLit = this.nightBlend > 0.5;
    this.world.relight();
    this.hover = null;
    this.remesh();
  }

  dispose() {
    this.disposed = true;
    cancelAnimationFrame(this.frame);
    this.resizeObserver.disconnect();
    this.visibilityObserver.disconnect();
    const { canvas } = this;
    canvas.removeEventListener("pointerdown", this.onPointerDown);
    canvas.removeEventListener("pointermove", this.onPointerMove);
    canvas.removeEventListener("pointerup", this.onPointerUp);
    canvas.removeEventListener("pointercancel", this.onPointerCancel);
    canvas.removeEventListener("pointerleave", this.onPointerLeave);
    canvas.removeEventListener("contextmenu", this.onContextMenu);
    // 不主动丢弃上下文：开发模式下 React 会卸载再挂载同一块画布，丢了就拿不回来。
    // The context is not lost on purpose: in development React unmounts and remounts the same
    // canvas, and a lost context cannot be recovered.
  }

  // ---- 资源 / Resources ----

  /** 把所有贴图叠成一个纹理数组，每张贴图一层，贴图之间不会互相渗色。 / Stacks every tile into one texture array, a layer per tile, so tiles never bleed into each other. */
  private createAtlas() {
    const { gl } = this;
    const layerBytes = TILE_SIZE * TILE_SIZE * 4;
    const pixels = new Uint8Array(layerBytes * TILE_NAMES.length);
    TILE_NAMES.forEach((name, layer) => pixels.set(tilePixels(name), layer * layerBytes));

    const texture = gl.createTexture();
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D_ARRAY, texture);
    gl.texImage3D(
      gl.TEXTURE_2D_ARRAY,
      0,
      gl.RGBA8,
      TILE_SIZE,
      TILE_SIZE,
      TILE_NAMES.length,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      pixels,
    );
    gl.generateMipmap(gl.TEXTURE_2D_ARRAY);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_MIN_FILTER, gl.NEAREST_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_MAX_LEVEL, 2);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_WRAP_S, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_WRAP_T, gl.REPEAT);
  }

  private createOutline(): WebGLVertexArrayObject {
    const { gl } = this;
    const array = gl.createVertexArray();
    const buffer = gl.createBuffer();
    if (!array || !buffer) {
      throw new Error("could not create the outline buffers");
    }
    gl.bindVertexArray(array);
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, cubeEdges(), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, 0);
    gl.bindVertexArray(null);
    return array;
  }

  private remesh() {
    const mesh = buildMesh(this.world);
    this.solid.upload(mesh.solid);
    this.water.upload(mesh.water);
    this.faces = mesh.faces;
    this.blocks = this.world.countBlocks();
  }

  // ---- 相机 / Camera ----

  private resize() {
    const { canvas } = this;
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    const width = Math.max(1, Math.round(canvas.clientWidth * ratio));
    const height = Math.max(1, Math.round(canvas.clientHeight * ratio));
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    this.aspect = width / height;
    // 宽屏时标题压在左边，把小岛往右挪；窄屏时标题在画布上方，小岛居中。
    // On wide screens the title covers the left, so the island moves right; on narrow screens
    // the title sits above the canvas and the island stays centred.
    this.centerShift = canvas.clientWidth >= 900 ? [0.36, 0.06] : [0, 0.08];
  }

  private updateCamera(): Float32Array {
    const half = Math.tan(FIELD_OF_VIEW / 2);
    const usableWidth = 1 - Math.abs(this.centerShift[0]);
    const distance = Math.max(FIT_VERTICAL / half, FIT_HORIZONTAL / (half * this.aspect * usableWidth)) * 1.08;
    const flat = Math.cos(this.pitch) * distance;
    this.eye = [
      WORLD_FOCUS[0] + Math.sin(this.yaw) * flat,
      WORLD_FOCUS[1] + Math.sin(this.pitch) * distance,
      WORLD_FOCUS[2] + Math.cos(this.yaw) * flat,
    ];
    this.basis = cameraBasis(this.eye, WORLD_FOCUS);
    const projection = perspective(
      FIELD_OF_VIEW,
      this.aspect,
      distance * 0.25,
      distance * 2.2,
      this.centerShift[0],
      this.centerShift[1],
    );
    return multiply(projection, lookAt(this.eye, this.basis));
  }

  /** 屏幕上一点对应的射线方向。 / The ray direction through a point on screen. */
  private rayThrough(clientX: number, clientY: number): Vec3 {
    const bounds = this.canvas.getBoundingClientRect();
    const half = Math.tan(FIELD_OF_VIEW / 2);
    const x = (((clientX - bounds.left) / bounds.width) * 2 - 1 - this.centerShift[0]) * half * this.aspect;
    const y = (1 - ((clientY - bounds.top) / bounds.height) * 2 - this.centerShift[1]) * half;
    const { forward, right, up } = this.basis;
    return normalize([
      forward[0] + right[0] * x + up[0] * y,
      forward[1] + right[1] * x + up[1] * y,
      forward[2] + right[2] * x + up[2] * y,
    ]);
  }

  private pick(clientX: number, clientY: number): RayHit | null {
    return castRay(this.world, this.eye, this.rayThrough(clientX, clientY));
  }

  // ---- 交互 / Interaction ----

  private readonly onPointerDown = (event: PointerEvent) => {
    if (this.pointerId !== null) {
      return;
    }
    this.pointerId = event.pointerId;
    this.pointerStart = [event.clientX, event.clientY];
    this.pointerLast = [event.clientX, event.clientY];
    this.dragging = false;
    this.idleFor = 0;
    this.canvas.setPointerCapture(event.pointerId);
  };

  private readonly onPointerMove = (event: PointerEvent) => {
    if (this.pointerId === event.pointerId) {
      const travelled = Math.hypot(event.clientX - this.pointerStart[0], event.clientY - this.pointerStart[1]);
      if (travelled > DRAG_THRESHOLD) {
        this.dragging = true;
      }
      if (this.dragging) {
        this.yaw -= (event.clientX - this.pointerLast[0]) * 0.008;
        this.pitch += (event.clientY - this.pointerLast[1]) * 0.006;
        this.pitch = Math.min(MAX_PITCH, Math.max(MIN_PITCH, this.pitch));
        this.hover = null;
      }
      this.pointerLast = [event.clientX, event.clientY];
      this.idleFor = 0;
      return;
    }
    if (event.pointerType === "mouse") {
      this.hover = this.pick(event.clientX, event.clientY);
    }
  };

  private readonly onPointerUp = (event: PointerEvent) => {
    if (this.pointerId !== event.pointerId) {
      return;
    }
    this.pointerId = null;
    this.idleFor = 0;
    if (this.dragging) {
      this.dragging = false;
      return;
    }
    const hit = this.pick(event.clientX, event.clientY);
    if (!hit) {
      return;
    }
    // 右键做和当前模式相反的事：挖掘模式下右键是放置。
    // The right button does the opposite of the current mode: in break mode it places.
    const placing = (this.mode === "place") !== (event.button === 2);
    if (placing) {
      this.placeBlock(hit);
    } else {
      this.breakBlock(hit);
    }
    this.hover = event.pointerType === "mouse" ? this.pick(event.clientX, event.clientY) : null;
  };

  private readonly onPointerCancel = (event: PointerEvent) => {
    if (this.pointerId === event.pointerId) {
      this.pointerId = null;
      this.dragging = false;
    }
  };

  private readonly onPointerLeave = () => {
    if (this.pointerId === null) {
      this.hover = null;
    }
  };

  private readonly onContextMenu = (event: Event) => {
    event.preventDefault();
  };

  private breakBlock(hit: RayHit) {
    const [x, y, z] = hit.block;
    this.world.set(x, y, z, AIR);
    // 竹子、草丛和花没了支撑会一起掉。
    // Bamboo, tufts and flowers break when their support is gone.
    for (let above = y + 1; ; above += 1) {
      const shape = BLOCK_BY_ID[this.world.get(x, above, z)]?.shape;
      if (shape !== "stalk" && shape !== "cross") {
        break;
      }
      this.world.set(x, above, z, AIR);
    }
    this.world.relight();
    this.remesh();
  }

  private placeBlock(hit: RayHit) {
    const x = hit.block[0] + hit.normal[0];
    const y = hit.block[1] + hit.normal[1];
    const z = hit.block[2] + hit.normal[2];
    if (!this.world.inBounds(x, y, z) || isOpaque(this.world.get(x, y, z))) {
      return;
    }
    this.world.set(x, y, z, this.selected);
    this.world.relight();
    this.remesh();
  }

  // ---- 每帧 / Per frame ----

  private schedule() {
    cancelAnimationFrame(this.frame);
    if (!this.disposed && this.visible) {
      this.frame = requestAnimationFrame(this.render);
    }
  }

  private readonly render = (now: number) => {
    const elapsed = this.lastTime === 0 ? 0 : Math.min(0.1, (now - this.lastTime) / 1000);
    this.lastTime = now;
    const motion = this.options.reducedMotion ? 0 : 1;
    this.clock += elapsed * motion;

    this.idleFor += elapsed;
    if (this.pointerId === null && this.idleFor > IDLE_BEFORE_SPIN) {
      this.yaw += AUTO_SPIN * elapsed * motion;
    }

    // 昼夜过渡：天色慢慢变，灯在过半时点亮。
    // The day–night blend: the light shifts gradually and lamps switch on halfway.
    const target = this.night ? 1 : 0;
    if (this.nightBlend !== target) {
      const step = elapsed / 0.9;
      this.nightBlend = target > this.nightBlend
        ? Math.min(target, this.nightBlend + step)
        : Math.max(target, this.nightBlend - step);
      const lit = this.nightBlend > 0.5;
      if (lit !== this.world.lampsLit) {
        this.world.lampsLit = lit;
        this.world.relight();
        this.remesh();
      }
    }

    this.draw();

    if (elapsed > 0) {
      this.fps += (1 / elapsed - this.fps) * 0.08;
    }
    this.sinceDebug += elapsed;
    if (this.sinceDebug >= 0.25) {
      this.sinceDebug = 0;
      const hover = this.hover;
      this.options.onDebug?.({
        fps: Math.round(this.fps),
        faces: this.faces,
        blocks: this.blocks,
        target: hover
          ? { position: hover.block, id: this.world.get(hover.block[0], hover.block[1], hover.block[2]) }
          : null,
      });
    }

    this.schedule();
  };

  private draw() {
    const { gl } = this;
    const viewProjection = this.updateCamera();
    const blend = this.nightBlend;
    const sky = DAY_LIGHT.map((day, index) => day + (NIGHT_LIGHT[index] - day) * blend);

    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.DEPTH_TEST);
    gl.enable(gl.CULL_FACE);
    gl.disable(gl.BLEND);
    gl.depthMask(true);

    gl.useProgram(this.program);
    gl.uniformMatrix4fv(this.uniform.viewProjection, false, viewProjection);
    gl.uniform1f(this.uniform.time, this.clock);
    gl.uniform1f(this.uniform.waterLayer, tileLayer("water"));
    gl.uniform1i(this.uniform.atlas, 0);
    gl.uniform3f(this.uniform.skyLight, sky[0], sky[1], sky[2]);
    gl.uniform3f(this.uniform.blockLight, LAMP_LIGHT[0], LAMP_LIGHT[1], LAMP_LIGHT[2]);
    gl.uniform1f(this.uniform.translucent, 0);
    this.solid.draw();

    // 画布是透明的，所以颜色按“预乘透明度”混合，叠在页面的天空上才不会发黑。
    // The canvas is transparent, so colours blend as premultiplied alpha; otherwise the water
    // would darken against the page's sky.
    gl.enable(gl.BLEND);
    gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.depthMask(false);
    gl.uniform1f(this.uniform.translucent, 1);
    this.water.draw();
    gl.depthMask(true);

    if (this.hover) {
      gl.useProgram(this.lineProgram);
      gl.uniformMatrix4fv(this.lineUniform.viewProjection, false, viewProjection);
      gl.uniform3f(this.lineUniform.origin, this.hover.block[0], this.hover.block[1], this.hover.block[2]);
      gl.uniform4f(this.lineUniform.color, 0, 0, 0, 0.85);
      gl.bindVertexArray(this.outline);
      gl.drawArrays(gl.LINES, 0, 24);
    }
    gl.disable(gl.BLEND);
    gl.bindVertexArray(null);
  }
}

/**
 * 在一块画布上启动体素场景。浏览器不支持 WebGL2 时返回 null，页面照常显示其它内容。
 *
 * Starts the voxel scene on a canvas. Returns null when the browser lacks WebGL2, and the
 * rest of the page still works.
 */
export function createVoxelEngine(canvas: HTMLCanvasElement, options: EngineOptions = {}): VoxelEngine | null {
  try {
    return new Engine(canvas, options);
  } catch (error) {
    console.warn("Voxel scene disabled:", error);
    return null;
  }
}
