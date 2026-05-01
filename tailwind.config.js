/** @type {import('tailwindcss').Config} */
module.exports = {
  // Scan all Askama templates so Tailwind knows which classes to keep
  content: ["./templates/**/*.html"],
  theme: {
    extend: {},
  },
  plugins: [],
};
