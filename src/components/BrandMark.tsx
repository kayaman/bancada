/** The Bancada mark, inlined from src-tauri/icons/brand/bancada-small.svg
 *  (the simplified ≤32px variant — the master's glow ring and bevel are
 *  sub-pixel at toolbar size). Inlined rather than imported: the brand
 *  masters live in the Rust crate's icon dir, outside the Vite root, and
 *  the frontend bundle must not reach into src-tauri. Keep the shapes in
 *  sync with the SVG if the mark is ever redrawn. */
export default function BrandMark({ size = 20 }: { size?: number }) {
  return (
    <svg viewBox="0 0 512 512" width={size} height={size} aria-hidden="true">
      <rect x="0" y="0" width="512" height="512" rx="115" fill="#1a1f2e" />
      {/* lamp */}
      <circle cx="256" cy="124" r="52" fill="#fbbf24" />
      {/* benchtop slab */}
      <rect x="56" y="216" width="400" height="96" fill="#2dd4bf" />
      {/* single square pulse step etched into the face */}
      <path
        d="M 96 282 H 208 V 246 H 300"
        fill="none"
        stroke="#1a1f2e"
        strokeWidth="22"
        strokeLinecap="butt"
        strokeLinejoin="miter"
      />
      {/* legs */}
      <rect x="104" y="312" width="68" height="144" fill="#2dd4bf" />
      <rect x="340" y="312" width="68" height="144" fill="#2dd4bf" />
    </svg>
  );
}
