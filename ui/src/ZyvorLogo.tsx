// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: Apache-2.0

type Props = {
  /** Show "zyvor" wordmark next to the mark */
  wordmark?: boolean
  /** Optional product line under/beside the wordmark */
  product?: string
  className?: string
  href?: string
  size?: number
}

/** Official Zyvor orange bracket-Z mark (zyvor.dev brand). */
export function ZyvorLogo({
  wordmark = true,
  product,
  className = '',
  href = 'https://zyvor.dev',
  size = 28,
}: Props) {
  const inner = (
    <>
      <img
        className="zyvor-mark"
        src="/zyvor-mark.svg"
        alt=""
        width={size}
        height={size}
        decoding="async"
      />
      {wordmark && (
        <span className="zyvor-wordmark">
          <span className="zyvor-name">zyvor</span>
          {product && <span className="zyvor-product">{product}</span>}
        </span>
      )}
    </>
  )

  if (href) {
    return (
      <a
        className={`zyvor-logo ${className}`.trim()}
        href={href}
        target="_blank"
        rel="noopener noreferrer"
        aria-label="Zyvor — zyvor.dev"
      >
        {inner}
      </a>
    )
  }

  return <span className={`zyvor-logo ${className}`.trim()}>{inner}</span>
}
