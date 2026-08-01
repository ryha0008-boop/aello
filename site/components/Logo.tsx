/** Three offset frames — one per environment sharing a repo. */
export default function Logo({ size = 22 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="1" y="1" width="13" height="13" rx="4" stroke="var(--hue-ops)" strokeWidth="1.4" />
      <rect
        x="5.5"
        y="5.5"
        width="13"
        height="13"
        rx="4"
        stroke="var(--hue-docs)"
        strokeWidth="1.4"
      />
      <rect x="10" y="10" width="13" height="13" rx="4" stroke="var(--hue-coder)" strokeWidth="1.4" />
    </svg>
  )
}
