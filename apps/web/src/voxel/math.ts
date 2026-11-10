export type Vec3 = [number, number, number];

/** 4×4 矩阵，按列存放，和 WebGL 的约定一致。 / A 4×4 matrix in column-major order, as WebGL expects. */
export type Mat4 = Float32Array;

export function subtract(a: Readonly<Vec3>, b: Readonly<Vec3>): Vec3 {
  return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
}

export function cross(a: Readonly<Vec3>, b: Readonly<Vec3>): Vec3 {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}

export function dot(a: Readonly<Vec3>, b: Readonly<Vec3>): number {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

export function normalize(vector: Readonly<Vec3>): Vec3 {
  const length = Math.hypot(vector[0], vector[1], vector[2]) || 1;
  return [vector[0] / length, vector[1] / length, vector[2] / length];
}

/** 透视投影。centerX 和 centerY 把画面中心在屏幕上平移，单位是裁剪空间。 / A perspective projection. `centerX` and `centerY` shift the image centre on screen, in clip-space units. */
export function perspective(
  fovY: number,
  aspect: number,
  near: number,
  far: number,
  centerX = 0,
  centerY = 0,
): Mat4 {
  const scale = 1 / Math.tan(fovY / 2);
  const matrix = new Float32Array(16);
  matrix[0] = scale / aspect;
  matrix[5] = scale;
  // 第三列乘的是 z，裁剪空间的 w 等于 -z，所以这里的偏移除以 w 之后正好是常数。
  // The third column multiplies z and clip-space w is -z, so after the divide this is a constant shift.
  matrix[8] = -centerX;
  matrix[9] = -centerY;
  matrix[10] = (far + near) / (near - far);
  matrix[11] = -1;
  matrix[14] = (2 * far * near) / (near - far);
  return matrix;
}

/** 相机坐标系：前、右、上三个互相垂直的单位向量。 / The camera frame: unit forward, right and up vectors. */
export interface CameraBasis {
  readonly forward: Vec3;
  readonly right: Vec3;
  readonly up: Vec3;
}

export function cameraBasis(eye: Readonly<Vec3>, target: Readonly<Vec3>): CameraBasis {
  const forward = normalize(subtract(target, eye));
  const right = normalize(cross(forward, [0, 1, 0]));
  const up = cross(right, forward);
  return { forward, right, up };
}

export function lookAt(eye: Readonly<Vec3>, basis: CameraBasis): Mat4 {
  const { forward, right, up } = basis;
  const matrix = new Float32Array(16);
  matrix[0] = right[0];
  matrix[1] = up[0];
  matrix[2] = -forward[0];
  matrix[4] = right[1];
  matrix[5] = up[1];
  matrix[6] = -forward[1];
  matrix[8] = right[2];
  matrix[9] = up[2];
  matrix[10] = -forward[2];
  matrix[12] = -dot(right, eye);
  matrix[13] = -dot(up, eye);
  matrix[14] = dot(forward, eye);
  matrix[15] = 1;
  return matrix;
}

export function multiply(a: Mat4, b: Mat4): Mat4 {
  const result = new Float32Array(16);
  for (let column = 0; column < 4; column += 1) {
    for (let row = 0; row < 4; row += 1) {
      let sum = 0;
      for (let k = 0; k < 4; k += 1) {
        sum += a[k * 4 + row] * b[column * 4 + k];
      }
      result[column * 4 + row] = sum;
    }
  }
  return result;
}
