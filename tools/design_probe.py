"""Render the HTML design prototype (index.html) in headless Edge and dump computed
geometry of the launcher, so the Slint port can be aligned against real numbers.

Usage:  python tools/design_probe.py [--size 860x560] [--out target/design_geom.txt]
"""
import argparse, html, os, re, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

PROBE_JS = r"""
<script>
window.addEventListener('load', function(){
  setTimeout(function(){
    var W = __W__, H = __H__;
    var sels = [
      '.search-window','.search-bar-wrapper','.search-icon-prefix','.search-input','.search-filter-icon-btn',
      '#searchFilterShelf','.filter-pill',
      '#pinnedShelfSection','.pinned-shelf-header','.pinned-toggle-btn','#pinnedTrackWrapper','#pinnedGrid','.pinned-card',
      '.pinned-card .squircle-icon','.pinned-card .pin-title','.pinned-card .pin-badge',
      '#categoryFilterBar','.category-tabs','.category-tab','.tab-icon','.tab-label',
      '#searchBody','#resultPane','.section-header',
      '.search-item','.search-item .squircle-icon','.item-main-col','.item-title','.item-badge','.item-path-col','.item-action-col',
      '#paletteFooter','.palette-footer-left','.footer-btn'
    ];
    var win = document.querySelector('.search-window').getBoundingClientRect();
    var out = [];
    out.push('VIEWPORT ' + window.innerWidth + 'x' + window.innerHeight + '  launcher=' + Math.round(win.width) + 'x' + Math.round(win.height));
    out.push('ALL COORDS BELOW ARE RELATIVE TO THE LAUNCHER TOP-LEFT (0,0)');
    sels.forEach(function(s){
      var els = document.querySelectorAll(s);
      if (!els.length) { out.push('MISSING ' + s); return; }
      out.push('=== ' + s + '  count=' + els.length + ' ===');
      var n = Math.min(els.length, 4);
      for (var i=0;i<n;i++){
        var e = els[i];
        var r = e.getBoundingClientRect();
        var cs = getComputedStyle(e);
        out.push('  ['+i+'] ' + Math.round(r.left-win.left) + ',' + Math.round(r.top-win.top)
          + ' ' + Math.round(r.width) + 'x' + Math.round(r.height)
          + ' | pad=' + cs.padding + ' mar=' + cs.margin + ' r=' + cs.borderRadius
          + ' | bg=' + cs.backgroundColor + ' bd=' + cs.borderTopWidth + ' ' + cs.borderTopColor
          + ' | fs=' + cs.fontSize + '/' + cs.fontWeight + ' c=' + cs.color
          + ' | gap=' + cs.gap + ' op=' + cs.opacity);
      }
    });
    var cats = document.querySelectorAll('.category-tab .tab-label');
    out.push('CATEGORY LABELS: ' + Array.prototype.map.call(cats, function(e){return e.textContent;}).join(' | '));
    var cards = document.querySelectorAll('.pinned-card');
    out.push('PINNED count=' + cards.length + ' titles=' + Array.prototype.map.call(cards, function(e){return (e.querySelector('.pin-title')||{textContent:'?'}).textContent;}).join(', '));
    var items = document.querySelectorAll('.search-item');
    out.push('RESULT rows=' + items.length + ' titles=' + Array.prototype.map.call(items, function(e){return (e.querySelector('.item-title')||{textContent:'?'}).textContent.trim();}).slice(0,12).join(' / '));
    var pre = document.createElement('pre'); pre.id = 'probe-out';
    pre.textContent = out.join('\n');
    document.body.innerHTML = ''; document.body.appendChild(pre);
    document.title = 'PROBE_DONE';
  }, 1500);
});
</script>
"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--size', default='860x560')
    ap.add_argument('--out', default=os.path.join(ROOT, 'target', 'design_geom.txt'))
    ap.add_argument('--png', default='')
    a = ap.parse_args()
    W, H = (int(v) for v in a.size.split('x'))

    src = open(os.path.join(ROOT, 'index.html'), encoding='utf-8').read()
    css = """
<style id="probe-css">
  .showcase-header, .desktop-icons-grid, .desktop-taskbar { display: none !important; }
  html, body { width: %dpx !important; height: %dpx !important; overflow: hidden !important; }
  .desktop-canvas { width: %dpx !important; height: %dpx !important;
                    background-image: none !important; background-color: #14171f !important; }
  :root { --launcher-width: %dpx !important; --launcher-height: %dpx !important; }
  * { animation: none !important; transition: none !important; }
</style>
""" % (W, H, W, H, W, H)
    js = PROBE_JS.replace('__W__', str(W)).replace('__H__', str(H))
    probe_path = os.path.join(ROOT, '_probe_dump.html')
    open(probe_path, 'w', encoding='utf-8').write(
        src.replace('</body>', css + js + '</body>'))

    cmd = [EDGE, '--headless=new', '--disable-gpu', '--hide-scrollbars',
           '--force-device-scale-factor=1', '--virtual-time-budget=8000',
           '--window-size=%d,%d' % (W, H), '--dump-dom', 'file:///' + probe_path.replace('\\', '/')]
    p = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8', errors='replace')
    dom = p.stdout or ''
    m = re.search(r'<pre id="probe-out">(.*?)</pre>', dom, re.S)
    if not m:
        print('PROBE FAILED; stdout len', len(dom), file=sys.stderr)
        open(a.out, 'w', encoding='utf-8').write('PROBE FAILED\n' + dom[:2000])
        return 1
    text = html.unescape(m.group(1))
    open(a.out, 'w', encoding='utf-8').write(text)
    print(text)
    return 0


if __name__ == '__main__':
    sys.exit(main())
