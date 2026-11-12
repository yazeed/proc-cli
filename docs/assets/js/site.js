// proc website: copy buttons, heading anchors, sidebar scroll tracking, and the command reader on the home page.
(function () {
  "use strict";

  function copyText(text, button) {
    var done = function () {
      var label = button.textContent;
      button.textContent = "Copied";
      button.classList.add("is-copied");
      setTimeout(function () {
        button.textContent = label;
        button.classList.remove("is-copied");
      }, 1600);
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(done, function () {});
    }
  }

  // The text of a code block as written: a block split into line boxes is
  // joined back line by line, since the browser's rendered text drops the
  // blank lines between them
  function codeText(pre) {
    var lines = pre.querySelectorAll("code > .line");
    if (!lines.length) return pre.innerText.replace(/\n$/, "");
    return Array.prototype.map.call(lines, function (line) { return line.textContent; }).join("\n");
  }

  // Copy buttons on every code block (and the home page install line)
  function addCopyButtons() {
    document.querySelectorAll(".prose pre, .term pre").forEach(function (pre) {
      var holder = pre.closest(".highlighter-rouge") || pre.parentElement;
      if (holder.querySelector(".copy-button")) return;
      var button = document.createElement("button");
      button.type = "button";
      button.className = "copy-button";
      button.textContent = "Copy";
      button.setAttribute("aria-label", "Copy code");
      button.addEventListener("click", function () {
        copyText(codeText(pre), button);
      });
      holder.style.position = "relative";
      holder.appendChild(button);
    });
    document.querySelectorAll("[data-copy]").forEach(function (button) {
      button.addEventListener("click", function () {
        copyText(button.getAttribute("data-copy"), button);
      });
    });
  }

  // Command blocks wrap rather than run off the edge (style.css). Each line
  // gets its own box, so a line that wraps keeps a hanging indent and reads as
  // one command. A block whose highlighted tokens span a line break can't be
  // split cleanly, so it is left whole: it still wraps, without the indent
  var COMMAND_BLOCKS = ".language-bash, .language-sh, .language-shell, .language-console, .language-bat, .language-powershell, .language-dockerfile, .commands";
  function splitCommandLines() {
    document.querySelectorAll(".prose div.highlighter-rouge").forEach(function (block) {
      if (!block.matches(COMMAND_BLOCKS)) return;
      var code = block.querySelector("pre code");
      if (!code || code.querySelector(".line")) return;
      var crosses = Array.prototype.some.call(code.children, function (el) {
        return el.textContent.indexOf("\n") !== -1;
      });
      if (crosses) return;
      var lines = [[]];
      Array.prototype.slice.call(code.childNodes).forEach(function (node) {
        if (node.nodeType !== 3) {
          lines[lines.length - 1].push(node);
          return;
        }
        node.textContent.split("\n").forEach(function (part, i) {
          if (i > 0) lines.push([]);
          if (part) lines[lines.length - 1].push(document.createTextNode(part));
        });
      });
      // the newline that ends the block
      if (lines.length > 1 && lines[lines.length - 1].length === 0) lines.pop();
      code.textContent = "";
      lines.forEach(function (nodes) {
        var line = document.createElement("span");
        line.className = "line";
        nodes.forEach(function (node) { line.appendChild(node); });
        code.appendChild(line);
      });
    });
  }

  // Nothing widens the page: a table scrolls inside its own box, and inline
  // code wraps between its words, never inside one (style.css). A word longer
  // than LONG_WORD may break anywhere, so a long path can't overflow a phone
  var LONG_WORD = 24;
  function fitWideContent() {
    document.querySelectorAll(".prose table").forEach(function (table) {
      if (table.parentElement.classList.contains("table-scroll")) return;
      var box = document.createElement("div");
      box.className = "table-scroll";
      table.parentNode.insertBefore(box, table);
      box.appendChild(table);
    });
    document.querySelectorAll(".prose :not(pre) > code").forEach(function (code) {
      if (code.children.length) return;
      var text = code.textContent;
      if (text.indexOf(" ") === -1) {
        if (text.length > LONG_WORD) code.classList.add("code-long");
        return;
      }
      code.classList.add("code-words");
      code.textContent = "";
      text.split(/( +)/).forEach(function (part) {
        if (!part) return;
        if (part.charAt(0) === " ") {
          code.appendChild(document.createTextNode(part));
          return;
        }
        var word = document.createElement("span");
        word.className = part.length > LONG_WORD ? "word word-long" : "word";
        word.textContent = part;
        code.appendChild(word);
      });
    });
  }

  // "#" links on headings that have ids, so sections can be shared
  function addHeadingAnchors() {
    document.querySelectorAll(".prose h2[id], .prose h3[id]").forEach(function (h) {
      var a = document.createElement("a");
      a.className = "heading-anchor";
      a.href = "#" + h.id;
      a.textContent = "#";
      a.setAttribute("aria-label", "Link to this section");
      h.appendChild(a);
    });
  }

  // Sidebars with data-spy mark the section being read, and keep that link in view
  function setupScrollSpy() {
    var nav = document.querySelector(".docs-nav[data-spy]");
    if (!nav) return;
    var aside = nav.closest(".page-aside");
    var links = {};
    nav.querySelectorAll('a[href^="#"]').forEach(function (a) {
      links[decodeURIComponent(a.getAttribute("href").slice(1))] = a;
    });
    var headings = Array.prototype.filter.call(document.querySelectorAll(".prose h2[id]"), function (h) {
      return links[h.id];
    });
    if (!headings.length) return;
    var current = null;
    function mark(id) {
      if (id === current) return;
      if (current && links[current]) links[current].removeAttribute("aria-current");
      current = id;
      var a = links[id];
      a.setAttribute("aria-current", "location");
      // scroll the sidebar itself, never the page
      if (aside && (a.offsetTop < aside.scrollTop || a.offsetTop + a.offsetHeight > aside.scrollTop + aside.clientHeight)) {
        aside.scrollTop = a.offsetTop - aside.clientHeight / 3;
      }
    }
    // the current section is the last heading that has scrolled past the top fifth of the screen;
    // at the very bottom it's the last one, since short final sections never reach that line
    function update() {
      var id = headings[0].id;
      var bottom = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 2;
      if (bottom) {
        id = headings[headings.length - 1].id;
      } else {
        var line = window.innerHeight / 5;
        for (var i = 0; i < headings.length; i++) {
          if (headings[i].getBoundingClientRect().top <= line) id = headings[i].id;
          else break;
        }
      }
      mark(id);
    }
    var queued = false;
    window.addEventListener("scroll", function () {
      if (queued) return;
      queued = true;
      requestAnimationFrame(function () {
        queued = false;
        update();
      });
    }, { passive: true });
    window.addEventListener("hashchange", update);
    update();
  }

  // The command reader: pick a command and a target, and read what that exact command does
  function setupReader() {
    var reader = document.querySelector("[data-reader]");
    if (!reader) return;
    var verbEl = reader.querySelector("[data-reader-verb]");
    var gapEl = reader.querySelector("[data-reader-gap]");
    var targetEl = reader.querySelector("[data-reader-target]");
    var explainEl = reader.querySelector("[data-reader-explain]");
    var verbsEl = reader.querySelector("[data-reader-verbs]");
    var targetsEl = reader.querySelector("[data-reader-targets]");
    var verbButtons = Array.prototype.slice.call(verbsEl.querySelectorAll("button"));
    var examples = {};
    reader.querySelectorAll("template[data-examples]").forEach(function (t) {
      examples[t.getAttribute("data-examples")] = t;
    });
    var timer = null;

    function targetButtons() {
      return Array.prototype.slice.call(targetsEl.querySelectorAll(":scope > button"));
    }

    function pickTarget(button) {
      var target = button.getAttribute("data-target");
      var parts = [];
      target.split(" ").forEach(function (word, i) {
        if (i) parts.push(" ");
        var span = document.createElement("span");
        span.className = "reader-word";
        span.textContent = word;
        parts.push(span);
      });
      targetEl.replaceChildren.apply(targetEl, parts);
      targetEl.hidden = gapEl.hidden = target === "";
      explainEl.replaceChildren(button.querySelector("template").content.cloneNode(true));
      targetButtons().forEach(function (b) {
        b.setAttribute("aria-pressed", b === button ? "true" : "false");
      });
    }

    // Switching command keeps the target when that command takes it, else uses its first example
    function pickVerb(verb, wantedTarget) {
      verbEl.textContent = verb;
      verbButtons.forEach(function (b) {
        b.setAttribute("aria-pressed", b.getAttribute("data-verb") === verb ? "true" : "false");
      });
      targetsEl.replaceChildren(examples[verb].content.cloneNode(true));
      var buttons = targetButtons();
      var match = buttons.filter(function (b) { return b.getAttribute("data-target") === wantedTarget; })[0];
      pickTarget(match || buttons[0]);
    }

    function stop() {
      if (timer) { clearInterval(timer); timer = null; }
    }

    verbsEl.addEventListener("click", function (e) {
      var b = e.target.closest("button");
      if (!b) return;
      stop();
      pickVerb(b.getAttribute("data-verb"), targetEl.textContent);
    });
    targetsEl.addEventListener("click", function (e) {
      var b = e.target.closest("button");
      if (!b) return;
      stop();
      pickTarget(b);
    });

    // Arrow keys move along a row and pick as they go
    [verbsEl, targetsEl].forEach(function (row) {
      row.addEventListener("keydown", function (e) {
        if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
        e.preventDefault();
        stop();
        var buttons = row === verbsEl ? verbButtons : targetButtons();
        var i = buttons.indexOf(document.activeElement);
        var next = buttons[(i + (e.key === "ArrowRight" ? 1 : buttons.length - 1)) % buttons.length];
        next.click();
        // picking a command rebuilds the target row, so find the button again
        (row === verbsEl ? next : targetButtons().filter(function (b) {
          return b.getAttribute("data-target") === next.getAttribute("data-target");
        })[0]).focus();
      });
    });

    pickVerb("kill", ":3000");

    // One short tour of different commands, once, unless motion is reduced
    var calm = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!calm) {
      var tour = [["on", ":3000"], ["why", ":3000"], ["free", ":3000"], ["stop", "node"],
        ["freeze", "node"], ["in", "."], ["orphans", "--in ."], ["kill", ":3000"]];
      var step = 0;
      timer = setInterval(function () {
        pickVerb(tour[step][0], tour[step][1]);
        step += 1;
        if (step >= tour.length) stop();
      }, 3400);
      ["pointerdown", "keydown", "focusin"].forEach(function (evt) {
        reader.addEventListener(evt, stop, { once: true });
      });
    }
  }

  document.addEventListener("DOMContentLoaded", function () {
    splitCommandLines();
    fitWideContent();
    addCopyButtons();
    addHeadingAnchors();
    setupScrollSpy();
    setupReader();
  });
})();
