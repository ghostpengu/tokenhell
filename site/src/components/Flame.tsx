import { useId } from 'react'

/** The TokenHell flame, same path as the app's header glyph. */
export function Flame({ className = '', solid }: { className?: string; solid?: string }) {
  const id = useId()
  return (
    <svg viewBox="0 0 100 100" className={className} aria-hidden>
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#ffc95c" />
          <stop offset="0.5" stopColor="#ff7a3a" />
          <stop offset="1" stopColor="#e5344a" />
        </linearGradient>
      </defs>
      <path
        d="M50 96C29 96 15 82 17 63 19 46 35 39 38 24 40 14 39 7 37 3 54 10 64 25 61 41 67 37 69 31 69 25 81 38 86 52 83 66 80 84 67 96 50 96Z"
        fill={solid ?? `url(#${id})`}
      />
    </svg>
  )
}
