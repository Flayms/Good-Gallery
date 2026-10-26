// Prints one line per bench result: node summarize.js results/a.json [...]
const fs = require('fs')
for (const f of process.argv.slice(2)) {
  const r = JSON.parse(fs.readFileSync(f, 'utf8')), p = r.play || {}, m = p.mpv || {}
  const seeks = r.seeks.filter((s) => s.first_frame_ms != null).map((s) => Math.round(s.first_frame_ms))
  const n = (x, d = 1) => (x == null ? '-' : x.toFixed(d))
  console.log(
    `${r.name.padEnd(26)} hw=${m.hwdec_current} ${JSON.stringify(p.render_size)} frames ${p.frames_rendered}/${p.frames_expected} ` +
      `drops vo=${p.vo_drops_in_run} dec=${p.decoder_drops_in_run} cpu1=${n(p.cpu_pct_of_one_core, 0)}% ` +
      `render p50/p95=${n(p.render_ms?.p50)}/${n(p.render_ms?.p95)} alpha=${n(p.alpha_fix_ms?.p50, 2)} write=${n(p.write_texture_ms?.p50, 2)} ` +
      `shown p95=${n(p.video_display_interval_ms?.p95)} judder=${p.video_judder} ui p95=${n(p.ui_interval_ms?.p95)} uicpu p95=${n(p.ui_cpu_ms?.p95, 2)} >20ms=${p.ui_intervals_over_20ms} avsync=${n(m.avsync, 4)} seek=${JSON.stringify(seeks)} err=${r.error}`,
  )
}
