import Link from "next/link";

/**
 * The ferret mark (docs/brand), drawn in the theme's `--brand` so it follows light and dark mode.
 * The view box ends at the mark's flat base, so it sits on the text baseline next to "ferret".
 */
export function BrandMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 3.4 32 24.75" aria-hidden className={className}>
      <path fill="var(--brand)" d="M2.06 28.15C1.625 22.495 1.915 17.13 4.09 13.36C5.25 11.33 6.845 9.88 8.875 9.01C7.425 7.125 7.57 4.805 9.31 3.935C11.05 3.065 12.935 4.08 13.805 6.11C16.995 5.385 20.185 6.11 22.36 7.85C23.955 9.155 25.985 9.88 28.74 10.605Q30.48 11.04 29.755 12.49C28.885 14.23 26.71 14.955 24.245 15.39L20.91 15.97C17.575 16.985 15.69 19.74 15.255 23.365L15.11 28.15H2.06Z" />
      <path fill="#f3ece2" d="M9.745 5.53C9.02 6.11 9.31 7.415 10.325 8.14L11.92 7.27C11.63 5.965 10.615 5.095 9.745 5.53Z" />
      <path fill="#f3ece2" d="M13.95 10.895C15.4 8.285 18.155 6.98 20.91 8.14C22.94 9.155 25.115 10.025 28.74 10.605Q30.48 11.04 29.755 12.49C28.885 14.23 26.71 14.955 24.245 15.39L20.91 15.97C18.01 16.985 16.415 19.015 15.69 20.9C15.98 18 14.965 15.535 13.95 13.795Q13.225 12.49 13.95 10.895Z" />
      <path fill="#4d2d15" d="M15.255 10.895C16.415 9.3 18.3 8.575 20.185 8.865C22.215 9.01 23.665 10.315 25.115 11.04C24.245 12.345 22.36 13.36 20.33 13.505C18.01 13.65 16.27 12.635 15.255 10.895Z" />
      <path fill="#1c110a" d="M28.74 10.605Q30.48 11.04 29.755 12.49L29.03 12.925Q28.015 12.2 28.74 10.605Z" />
      <path fill="#1c110a" d="M21.722 10.46C21.722 9.9715 21.3585 9.5755 20.91 9.5755C20.4615 9.5755 20.098 9.9715 20.098 10.46C20.098 10.9485 20.4615 11.3445 20.91 11.3445C21.3585 11.3445 21.722 10.9485 21.722 10.46Z" />
      <path fill="#f3ece2" d="M21.0115 10.199C21.0115 10.0308 20.8752 9.8945 20.707 9.8945C20.5388 9.8945 20.4025 10.0308 20.4025 10.199C20.4025 10.3672 20.5388 10.5035 20.707 10.5035C20.8752 10.5035 21.0115 10.3672 21.0115 10.199Z" />
    </svg>
  );
}

/** "ferret" with the mark, linking home: the top-left of every page with a header. */
export function HomeLink() {
  return (
    <Link href="/" className="inline-flex items-baseline gap-1.5 font-wordmark text-2xl font-semibold text-brand select-none">
      <BrandMark className="h-[0.8em] w-auto" />
      ferret
    </Link>
  );
}
