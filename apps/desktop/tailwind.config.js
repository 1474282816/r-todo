/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "media", // 跟随系统
  theme: {
    extend: {
      colors: {
        // 优先级色（与 packages/design-tokens/colors.json 保持一致）
        priority: {
          high: "#ef4444",
          medium: "#f59e0b",
          low: "#9ca3af",
        },
      },
      fontFamily: {
        sans: ['"Segoe UI"', "system-ui", "sans-serif"],
      },
    },
  },
  plugins: [],
};
