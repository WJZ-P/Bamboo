/** 图标画布的边长。三个图标都画在同一块画布上，大小和位置才一致。 */
const CANVAS_SIZE = 12;

/** 一个像素方块：[x, y, 宽, 高]。 */
type PixelRect = readonly [x: number, y: number, width: number, height: number];

/** 用方块拼出的图标。字体字符的大小和位置取决于字形，没法互相对齐，所以这里直接画。 */
function PixelIcon({ rects }: { rects: readonly PixelRect[] }) {
  return (
    <svg
      width={CANVAS_SIZE}
      height={CANVAS_SIZE}
      viewBox={`0 0 ${CANVAS_SIZE} ${CANVAS_SIZE}`}
      fill="currentColor"
      shapeRendering="crispEdges"
      aria-hidden="true"
    >
      {rects.map(([x, y, width, height]) => (
        <rect key={`${x},${y}`} x={x} y={y} width={width} height={height} />
      ))}
    </svg>
  );
}

const MINIMIZE_RECTS: readonly PixelRect[] = [[1, 5, 10, 2]];

const MAXIMIZE_RECTS: readonly PixelRect[] = [
  [1, 1, 10, 2],
  [1, 9, 10, 2],
  [1, 3, 2, 6],
  [9, 3, 2, 6],
];

/** 两条对角线共用中间的方块。 */
const CLOSE_RECTS: readonly PixelRect[] = [
  [1, 1, 2, 2],
  [3, 3, 2, 2],
  [5, 5, 2, 2],
  [7, 7, 2, 2],
  [9, 9, 2, 2],
  [9, 1, 2, 2],
  [7, 3, 2, 2],
  [3, 7, 2, 2],
  [1, 9, 2, 2],
];

export function MinimizeIcon() {
  return <PixelIcon rects={MINIMIZE_RECTS} />;
}

export function MaximizeIcon() {
  return <PixelIcon rects={MAXIMIZE_RECTS} />;
}

export function CloseIcon() {
  return <PixelIcon rects={CLOSE_RECTS} />;
}
