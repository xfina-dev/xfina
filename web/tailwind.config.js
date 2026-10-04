import xfina from 'xfina-ui/tailwind';

// The preset brings every colour, the radius and dark mode; Xfina defines none.
// The last content path is required: without it Tailwind never sees the
// classes inside xfina-ui's components and they render unstyled.
export default {
  presets: [xfina],
  content: [
    './index.html',
    './src/**/*.{vue,js}',
    './node_modules/xfina-ui/dist/**/*.js',
  ],
};
