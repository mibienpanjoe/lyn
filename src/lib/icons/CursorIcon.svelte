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
  viewBox="0 0 28 28"
  fill="none"
  xmlns="http://www.w3.org/2000/svg"
  class={className}
  aria-hidden="true"
>
  <defs>
    <linearGradient
      id="cursor-grad-left"
      x1="13.0148"
      y1="0.457031"
      x2="13.0148"
      y2="17.0116"
      gradientUnits="userSpaceOnUse"
    >
      <stop stop-color="#38bdf8" />
      <stop offset="1" stop-color="#0284c7" />
    </linearGradient>
    <linearGradient
      id="cursor-grad-right"
      x1="19.4605"
      y1="0.457031"
      x2="19.4605"
      y2="27.3078"
      gradientUnits="userSpaceOnUse"
    >
      <stop stop-color="#0284c7" />
      <stop offset="1" stop-color="#0369a1" />
    </linearGradient>
  </defs>
  <g transform="translate(1, 1)">
    <path
      d="M22.8416 1.387C23.4132 1.0062 24.1735 1.44515 24.1295 2.13055L22.6326 25.4656C22.5957 26.0405 21.8646 26.2609 21.5162 25.8022L15.0114 17.2395C14.8429 17.0177 14.5923 16.873 14.316 16.838L3.6481 15.486C3.07656 15.4136 2.9019 14.6703 3.38136 14.3509L22.8416 1.387Z"
      fill="#0f172a"
      opacity="0.3"
    />
    <path
      d="M1.79102 15.2782L24.2386 0.457031L14.6808 17.0116L1.79102 15.2782Z"
      fill="url(#cursor-grad-left)"
    />
    <path
      d="M22.6278 27.3078L24.2394 0.457031L14.6816 17.0116L22.6278 27.3078Z"
      fill="url(#cursor-grad-right)"
    />
    <path
      d="M24.2386 0.457031L14.6808 17.0116"
      stroke="#ffffff"
      stroke-width="0.6"
      stroke-opacity="0.75"
    />
  </g>
</svg>
