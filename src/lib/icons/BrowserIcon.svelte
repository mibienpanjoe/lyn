<script lang="ts">
  interface Props {
    size?: number;
    class?: string;
  }
  let { size = 20, class: className = '' }: Props = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 32 32"
  fill="none"
  xmlns="http://www.w3.org/2000/svg"
  class={className}
  aria-hidden="true"
>
  <rect
    x="2"
    y="3"
    width="28"
    height="26"
    rx="6"
    fill="#1e293b"
    stroke="#334155"
    stroke-width="1.5"
  />
  <path d="M2 9.5H30" stroke="#334155" stroke-width="1.5" />
  <circle cx="6" cy="6.2" r="1.5" fill="#ea4335" />
  <circle cx="10" cy="6.2" r="1.5" fill="#fbbc05" />
  <circle cx="14" cy="6.2" r="1.5" fill="#34a853" />
  <rect
    x="18"
    y="4.7"
    width="10"
    height="3"
    rx="1.5"
    fill="#334155"
    opacity="0.7"
  />
  <!-- Globe / compass grid representing multi-browser navigation -->
  <circle
    cx="16"
    cy="20"
    r="7.5"
    stroke="#38bdf8"
    stroke-width="1.5"
    fill="#0f172a"
  />
  <ellipse
    cx="16"
    cy="20"
    rx="3.7"
    ry="7.5"
    stroke="#38bdf8"
    stroke-width="1.2"
    stroke-dasharray="2 1"
  />
  <path d="M8.5 20H23.5" stroke="#38bdf8" stroke-width="1.2" />
  <!-- Vibrant browser family dots on the sides: Chrome green, Firefox orange, Edge cyan -->
  <circle cx="21" cy="15" r="1.8" fill="#ff7a00" />
  <circle cx="11" cy="25" r="1.8" fill="#38bdf8" />
</svg>
