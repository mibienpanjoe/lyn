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
    y="4"
    width="28"
    height="24"
    rx="5"
    fill="#1e293b"
    stroke="#334155"
    stroke-width="1.5"
  />
  <rect x="2" y="4" width="28" height="7" rx="5" fill="#0f172a" />
  <circle cx="6.5" cy="7.5" r="1.5" fill="#ef4444" />
  <circle cx="11" cy="7.5" r="1.5" fill="#f59e0b" />
  <circle cx="15.5" cy="7.5" r="1.5" fill="#10b981" />
  <path
    d="M7 16L12 20L7 24"
    stroke="#38bdf8"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
  />
  <path
    d="M15 24H24"
    stroke="#a855f7"
    stroke-width="2"
    stroke-linecap="round"
  />
</svg>
