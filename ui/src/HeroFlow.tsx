// Copyright 2026 Zyvor AI Labs
// SPDX-License-Identifier: Apache-2.0

export function HeroFlow() {
  return (
    <div className="hero-flow" aria-hidden>
      <svg viewBox="0 0 1140 140" preserveAspectRatio="none">
        <path
          d="M 40 70 C 220 10, 380 130, 560 70 S 900 20, 1100 70"
          fill="none"
          stroke="rgba(0,0,0,0.08)"
          strokeWidth="1.5"
        />
      </svg>
      <span className="flow-dot flow-dot-a" />
      <span className="flow-dot flow-dot-b" />
      <span className="flow-dot flow-dot-c" />
    </div>
  )
}
