/* Radial Launcher site — carousel, copy-to-clipboard, and the hero wheel. */
(function () {
    "use strict";
    const $ = (id) => document.getElementById(id);

    /* ---------- carousel ---------- */
    const track = $("track");
    const stage = track && track.parentElement;
    const slides = track ? Array.from(track.children) : [];
    const dots = [$("dot-0"), $("dot-1")];
    let slide = 0;

    function fitHeight() {   // clip the stage to the active slide (slides differ in height)
        if (stage && slides[slide]) stage.style.height = slides[slide].offsetHeight + "px";
    }
    function go(i) {
        slide = Math.max(0, Math.min(slides.length - 1, i));
        if (track) track.style.transform = `translateX(-${slide * 100}%)`;
        dots.forEach((d, n) => d && d.classList.toggle("is-active", n === slide));
        fitHeight();
    }

    fitHeight();
    addEventListener("resize", fitHeight);
    addEventListener("load", fitHeight);
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(fitHeight);

    $("brand")        && $("brand").addEventListener("click", () => go(0));
    $("nav-features") && $("nav-features").addEventListener("click", () => go(1));
    $("see-features") && $("see-features").addEventListener("click", () => go(1));
    dots.forEach((d, n) => d && d.addEventListener("click", () => go(n)));

    document.addEventListener("keydown", (e) => {
        if (e.key === "ArrowRight") go(slide + 1);
        else if (e.key === "ArrowLeft") go(slide - 1);
    });

    /* ---------- copy install command ---------- */
    const copyBtn = $("copy"), cmd = $("cmd");
    if (copyBtn && cmd) {
        const original = copyBtn.innerHTML;
        copyBtn.addEventListener("click", async () => {
            try {
                await navigator.clipboard.writeText(cmd.textContent.trim());
            } catch (_) {
                const r = document.createRange(); r.selectNodeContents(cmd);
                const s = getSelection(); s.removeAllRanges(); s.addRange(r);
                try { document.execCommand("copy"); } catch (e) {}
                s.removeAllRanges();
            }
            copyBtn.classList.add("done");
            copyBtn.innerHTML = '<svg class="ic"><use href="#i-check"/></svg>';
            setTimeout(() => { copyBtn.classList.remove("done"); copyBtn.innerHTML = original; }, 1500);
        });
    }

    /* ---------- hero wheel ---------- */
    const wheel = $("wheel");
    const pill = $("hub-pill");
    if (wheel) {
        const tiles = Array.from(wheel.querySelectorAll(".tile"));
        const step = 360 / tiles.length;
        let idx = 0, angle = 0, timer = null;

        function select(i) {
            idx = (i + tiles.length) % tiles.length;
            // accumulate the angle, always rotating the short way (no full-circle spin at wrap)
            const target = idx * step;
            let cur = ((angle % 360) + 360) % 360;
            let delta = ((target - cur) % 360 + 540) % 360 - 180;
            angle += delta;
            wheel.style.setProperty("--a", angle + "deg");
            tiles.forEach((t, n) => t.classList.toggle("on", n === idx));
            if (pill) pill.textContent = tiles[idx].dataset.name || "";
        }
        function play()  { stop(); timer = setInterval(() => select(idx + 1), 2200); }
        function stop()  { if (timer) { clearInterval(timer); timer = null; } }

        tiles.forEach((t, i) => {
            t.addEventListener("mouseenter", () => { stop(); select(i); });
            t.addEventListener("focus", () => { stop(); select(i); });
        });
        wheel.addEventListener("mouseleave", play);

        select(0);
        play();
    }
})();
