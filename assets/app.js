/* 4d-catalog-diagram — interactive shell.
 *
 * Vanilla JS against the SVG DOM: no framework, no network, no build step.
 * Everything degrades to the server-rendered static SVG if this never runs. */
(function () {
  "use strict";

  var data = JSON.parse(document.getElementById("diagram-data").textContent);
  var canvas = document.getElementById("canvas");
  var viewport = document.getElementById("viewport");
  var stage = document.getElementById("stage");
  var popup = document.getElementById("popup");
  var search = document.getElementById("search");
  var list = document.getElementById("table-list");
  var minimap = document.getElementById("minimap");
  var minimapCards = document.getElementById("minimap-cards");
  var minimapView = document.getElementById("minimap-view");

  /* Below this many tables a minimap is clutter rather than navigation. */
  var MINIMAP_MIN_CARDS = 16;
  var MINIMAP_W = 180;
  var MINIMAP_H = 120;

  var state = { x: 0, y: 0, k: 1, layout: data.default.layout, hide: data.default.hide_system_fields };

  function activeScene() {
    return canvas.querySelector(".scene.scene-active");
  }

  function sceneFor(layout, hide) {
    var selector = '.scene[data-layout="' + layout + '"][data-hide="' + (hide ? 1 : 0) + '"]';
    return canvas.querySelector(selector);
  }

  /* ------------------------------------------------------------ viewport */

  function applyTransform() {
    viewport.setAttribute(
      "transform",
      "translate(" + state.x.toFixed(2) + " " + state.y.toFixed(2) + ") scale(" + state.k.toFixed(4) + ")"
    );
    updateMinimapView();
  }

  function sizeCanvas() {
    var rect = stage.getBoundingClientRect();
    canvas.setAttribute("viewBox", "0 0 " + Math.max(1, Math.round(rect.width)) + " " + Math.max(1, Math.round(rect.height)));
  }

  function fit() {
    var scene = activeScene();
    if (!scene) return;
    var rect = stage.getBoundingClientRect();
    var w = parseFloat(scene.getAttribute("data-width"));
    var h = parseFloat(scene.getAttribute("data-height"));
    if (!w || !h || !rect.width || !rect.height) return;
    state.k = Math.min(rect.width / w, rect.height / h) * 0.94;
    state.k = Math.max(0.05, Math.min(4, state.k));
    state.x = (rect.width - w * state.k) / 2;
    state.y = (rect.height - h * state.k) / 2;
    applyTransform();
  }

  function zoomAt(clientX, clientY, factor) {
    var rect = stage.getBoundingClientRect();
    var px = clientX - rect.left;
    var py = clientY - rect.top;
    var next = Math.max(0.05, Math.min(8, state.k * factor));
    var ratio = next / state.k;
    state.x = px - (px - state.x) * ratio;
    state.y = py - (py - state.y) * ratio;
    state.k = next;
    applyTransform();
  }

  function centerOn(card) {
    var rect = stage.getBoundingClientRect();
    var match = /translate\(\s*(-?[\d.]+)[ ,]+(-?[\d.]+)\s*\)/.exec(card.getAttribute("transform") || "");
    if (!match) return;
    var cx = parseFloat(match[1]);
    var cy = parseFloat(match[2]);
    var box = card.getBBox();
    state.k = Math.max(state.k, 0.9);
    state.x = rect.width / 2 - (cx + box.width / 2) * state.k;
    state.y = rect.height / 2 - (cy + box.height / 2) * state.k;
    applyTransform();
  }

  /* ------------------------------------------------------------ selection */

  function clearSelection() {
    var scene = activeScene();
    if (!scene) return;
    scene.classList.remove("has-selection");
    scene.querySelectorAll(".is-selected, .is-related").forEach(function (el) {
      el.classList.remove("is-selected", "is-related");
    });
    list.querySelectorAll(".is-selected").forEach(function (el) { el.classList.remove("is-selected"); });
    scene.querySelectorAll(".is-field-target").forEach(function (el) { el.classList.remove("is-field-target"); });
    // An expanded card draws over its neighbours, so it must not outlive the
    // selection that justified it.
    scene.querySelectorAll(".card.is-expanded").forEach(function (el) { el.classList.remove("is-expanded"); });
    minimapCards.querySelectorAll(".is-selected").forEach(function (el) { el.classList.remove("is-selected"); });
    popup.hidden = true;
  }

  function selectTable(name) {
    var scene = activeScene();
    if (!scene) return;
    clearSelection();
    var card = scene.querySelector('.card[data-table="' + cssEscape(name) + '"]');
    if (!card) return;
    scene.classList.add("has-selection");
    card.classList.add("is-selected", "is-related");
    scene.querySelectorAll(".edge").forEach(function (edge) {
      if (edge.getAttribute("data-from") === name || edge.getAttribute("data-to") === name) {
        edge.classList.add("is-related");
        var other = edge.getAttribute("data-from") === name ? edge.getAttribute("data-to") : edge.getAttribute("data-from");
        var neighbour = scene.querySelector('.card[data-table="' + cssEscape(other) + '"]');
        if (neighbour) neighbour.classList.add("is-related");
      }
    });
    var item = list.querySelector('.table-item[data-target="' + cssEscape(name) + '"]');
    if (item) {
      item.classList.add("is-selected");
      item.scrollIntoView({ block: "nearest" });
    }
    var pip = minimapCards.querySelector('[data-table="' + cssEscape(name) + '"]');
    if (pip) pip.classList.add("is-selected");
    return card;
  }

  /* --------------------------------------------------------- deep linking */

  /* `file.html#CLIENTS` centres on a table; `file.html#CLIENTS.ID` also
   * highlights one field. Purely client-side, so it works from file://. */
  function parseHash(hash) {
    var raw = String(hash || "").replace(/^#/, "");
    if (!raw) return null;
    try { raw = decodeURIComponent(raw); } catch (e) { /* keep the raw form */ }
    var dot = raw.indexOf(".");
    if (dot === -1) return { table: raw, field: null };
    return { table: raw.slice(0, dot), field: raw.slice(dot + 1) };
  }

  var suppressHashChange = false;

  function writeHash(table, field) {
    var next = table ? "#" + encodeURIComponent(table) + (field ? "." + encodeURIComponent(field) : "") : "";
    var url = location.pathname + location.search + next;
    if (location.hash === next || (!next && !location.hash)) return;
    suppressHashChange = true;
    if (history.replaceState) history.replaceState(null, "", url);
    else location.hash = next;
    setTimeout(function () { suppressHashChange = false; }, 0);
  }

  function applyHash() {
    var target = parseHash(location.hash);
    if (!target) return false;
    var card = selectTable(target.table);
    if (!card) return false;
    revealField(card, target.field);
    centerOn(card);
    return true;
  }

  function revealField(card, field) {
    if (!field) return;
    var row = card.querySelector('.row[data-field="' + cssEscape(field) + '"]');
    if (!row) return;
    // The field may be behind the "Show N more" cut.
    if (card.contains(row) && row.closest(".card-extra-rows")) expandCard(card, true);
    row.classList.add("is-field-target");
  }

  /* ------------------------------------------------ progressive disclosure */

  function expandCard(card, expand) {
    var scene = activeScene();
    if (!scene) return;
    if (expand) {
      // Only one card at a time, since an expanded card draws over its
      // neighbours rather than pushing them aside.
      scene.querySelectorAll(".card.is-expanded").forEach(function (other) {
        if (other !== card) other.classList.remove("is-expanded");
      });
      card.classList.add("is-expanded");
      // SVG has no z-index: raise the card by making it the last sibling.
      card.parentNode.appendChild(card);
    } else {
      card.classList.remove("is-expanded");
    }
  }

  function cssEscape(value) {
    if (window.CSS && CSS.escape) return CSS.escape(value);
    return String(value).replace(/["\\\]\[]/g, "\\$&");
  }

  function relationFor(id) {
    var variant = data.variants.filter(function (v) {
      return v.layout === state.layout && v.hide_system_fields === state.hide;
    })[0] || data.variants[0];
    return variant.relations.filter(function (r) { return r.id === id; })[0];
  }

  function showRelation(edge, event) {
    var relation = relationFor(edge.getAttribute("data-edge"));
    if (!relation) return;
    var rows = [
      ["Many → one", relation.from_table + "." + relation.from_field + " → " + relation.to_table + "." + relation.to_field],
      ["N→1 name", relation.name_Nto1 || "—"],
      ["1→N name", relation.name_1toN || "—"]
    ];
    if (relation.integrity) rows.push(["Integrity", relation.integrity]);
    rows.push(["Auto load", (relation.auto_load_Nto1 ? "N→1" : "") + (relation.auto_load_1toN ? (relation.auto_load_Nto1 ? ", 1→N" : "1→N") : "") || "no"]);

    var html = "<h2></h2><dl></dl>";
    popup.innerHTML = html;
    popup.querySelector("h2").textContent = relation.from_table + " → " + relation.to_table;
    var dl = popup.querySelector("dl");
    rows.forEach(function (row) {
      var dt = document.createElement("dt");
      dt.textContent = row[0];
      var dd = document.createElement("dd");
      dd.textContent = row[1];
      dl.appendChild(dt);
      dl.appendChild(dd);
    });

    var rect = stage.getBoundingClientRect();
    popup.hidden = false;
    var w = popup.offsetWidth;
    var h = popup.offsetHeight;
    popup.style.left = Math.min(rect.width - w - 8, Math.max(8, event.clientX - rect.left + 12)) + "px";
    popup.style.top = Math.min(rect.height - h - 8, Math.max(8, event.clientY - rect.top + 12)) + "px";
  }

  /* --------------------------------------------------------------- events */

  var dragging = false;
  var last = null;

  canvas.addEventListener("pointerdown", function (event) {
    var edge = event.target.closest(".edge");
    if (edge) {
      event.stopPropagation();
      clearSelection();
      var scene = activeScene();
      scene.classList.add("has-selection");
      edge.classList.add("is-related");
      [edge.getAttribute("data-from"), edge.getAttribute("data-to")].forEach(function (name) {
        var card = scene.querySelector('.card[data-table="' + cssEscape(name) + '"]');
        if (card) card.classList.add("is-related");
      });
      showRelation(edge, event);
      return;
    }
    var more = event.target.closest(".row-more");
    if (more) {
      event.stopPropagation();
      var owner = more.closest(".card");
      expandCard(owner, !owner.classList.contains("is-expanded"));
      return;
    }
    var card = event.target.closest(".card");
    if (card) {
      event.stopPropagation();
      var name = card.getAttribute("data-table");
      var row = event.target.closest(".row");
      var field = row ? row.getAttribute("data-field") : null;
      var selected = selectTable(name);
      if (selected) revealField(selected, field);
      writeHash(name, field);
      return;
    }
    dragging = true;
    last = { x: event.clientX, y: event.clientY };
    canvas.classList.add("is-panning");
    canvas.setPointerCapture(event.pointerId);
    clearSelection();
    writeHash(null, null);
  });

  canvas.addEventListener("pointermove", function (event) {
    if (!dragging) return;
    state.x += event.clientX - last.x;
    state.y += event.clientY - last.y;
    last = { x: event.clientX, y: event.clientY };
    applyTransform();
  });

  ["pointerup", "pointercancel"].forEach(function (type) {
    canvas.addEventListener(type, function () {
      dragging = false;
      canvas.classList.remove("is-panning");
    });
  });

  canvas.addEventListener("wheel", function (event) {
    event.preventDefault();
    zoomAt(event.clientX, event.clientY, event.deltaY < 0 ? 1.12 : 1 / 1.12);
  }, { passive: false });

  document.getElementById("zoom-in").addEventListener("click", function () {
    var rect = stage.getBoundingClientRect();
    zoomAt(rect.left + rect.width / 2, rect.top + rect.height / 2, 1.25);
  });
  document.getElementById("zoom-out").addEventListener("click", function () {
    var rect = stage.getBoundingClientRect();
    zoomAt(rect.left + rect.width / 2, rect.top + rect.height / 2, 1 / 1.25);
  });
  document.getElementById("zoom-fit").addEventListener("click", fit);

  list.addEventListener("click", function (event) {
    var button = event.target.closest(".table-item");
    if (!button) return;
    var name = button.getAttribute("data-target");
    var card = selectTable(name);
    if (card) {
      centerOn(card);
      writeHash(name, null);
    }
  });

  search.addEventListener("input", function () {
    var query = search.value.trim().toLowerCase();
    var scene = activeScene();
    list.querySelectorAll(".table-item").forEach(function (item) {
      var name = item.getAttribute("data-target").toLowerCase();
      item.parentElement.hidden = query !== "" && name.indexOf(query) === -1;
    });
    if (!scene) return;
    scene.classList.toggle("has-query", query !== "");
    scene.querySelectorAll(".card").forEach(function (card) {
      var name = (card.getAttribute("data-table") || "").toLowerCase();
      card.classList.toggle("is-match", query !== "" && name.indexOf(query) !== -1);
    });
  });

  document.addEventListener("keydown", function (event) {
    if (event.key === "Escape") {
      clearSelection();
      if (document.activeElement === search) search.blur();
    }
    if (event.key === "/" && document.activeElement !== search) {
      event.preventDefault();
      search.focus();
    }
  });

  function switchScene() {
    var next = sceneFor(state.layout, state.hide);
    if (!next) return;
    clearSelection();
    canvas.querySelectorAll(".card.is-expanded").forEach(function (card) {
      card.classList.remove("is-expanded");
    });
    canvas.querySelectorAll(".scene").forEach(function (scene) {
      scene.classList.toggle("scene-active", scene === next);
    });
    search.dispatchEvent(new Event("input"));
    buildMinimap();
    fit();
    applyHash();
  }

  /* ------------------------------------------------------------- minimap */

  var minimapScale = 0;

  function buildMinimap() {
    var scene = activeScene();
    minimapCards.textContent = "";
    if (!scene) return;
    var cards = scene.querySelectorAll(".card");
    if (cards.length < MINIMAP_MIN_CARDS) {
      minimap.hidden = true;
      return;
    }
    var w = parseFloat(scene.getAttribute("data-width"));
    var h = parseFloat(scene.getAttribute("data-height"));
    minimapScale = Math.min(MINIMAP_W / w, MINIMAP_H / h);
    cards.forEach(function (card) {
      var match = /translate\(\s*(-?[\d.]+)[ ,]+(-?[\d.]+)\s*\)/.exec(card.getAttribute("transform") || "");
      if (!match) return;
      var box = card.querySelector(".card-bg, .ghost-bg");
      if (!box) return;
      var pip = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      pip.setAttribute("class", "mm-card");
      pip.setAttribute("data-table", card.getAttribute("data-table"));
      pip.setAttribute("x", (parseFloat(match[1]) * minimapScale).toFixed(2));
      pip.setAttribute("y", (parseFloat(match[2]) * minimapScale).toFixed(2));
      pip.setAttribute("width", Math.max(1, parseFloat(box.getAttribute("width")) * minimapScale).toFixed(2));
      pip.setAttribute("height", Math.max(1, parseFloat(box.getAttribute("height")) * minimapScale).toFixed(2));
      minimapCards.appendChild(pip);
    });
    minimap.hidden = false;
    updateMinimapView();
  }

  function updateMinimapView() {
    if (minimap.hidden || !minimapScale) return;
    var rect = stage.getBoundingClientRect();
    // Diagram-space rectangle currently on screen, mapped into minimap space.
    var ratio = minimapScale / state.k;
    minimapView.setAttribute("x", (-state.x * ratio).toFixed(2));
    minimapView.setAttribute("y", (-state.y * ratio).toFixed(2));
    minimapView.setAttribute("width", Math.max(2, rect.width * ratio).toFixed(2));
    minimapView.setAttribute("height", Math.max(2, rect.height * ratio).toFixed(2));
  }

  function minimapPanTo(event) {
    if (!minimapScale) return;
    var box = minimap.getBoundingClientRect();
    var rect = stage.getBoundingClientRect();
    var dx = (event.clientX - box.left) / minimapScale;
    var dy = (event.clientY - box.top) / minimapScale;
    state.x = rect.width / 2 - dx * state.k;
    state.y = rect.height / 2 - dy * state.k;
    applyTransform();
  }

  var minimapDragging = false;
  minimap.addEventListener("pointerdown", function (event) {
    event.preventDefault();
    minimapDragging = true;
    try { minimap.setPointerCapture(event.pointerId); } catch (e) { /* no capture, still drags */ }
    minimapPanTo(event);
  });
  minimap.addEventListener("pointermove", function (event) {
    if (minimapDragging) minimapPanTo(event);
  });
  ["pointerup", "pointercancel"].forEach(function (type) {
    minimap.addEventListener(type, function () { minimapDragging = false; });
  });

  window.addEventListener("hashchange", function () {
    if (suppressHashChange) return;
    applyHash();
  });

  var toggleSystem = document.getElementById("toggle-system");
  if (toggleSystem && !toggleSystem.disabled) {
    toggleSystem.addEventListener("change", function () {
      state.hide = toggleSystem.checked;
      switchScene();
    });
  }

  var toggleLayout = document.getElementById("toggle-layout");
  if (toggleLayout && !toggleLayout.disabled) {
    toggleLayout.addEventListener("change", function () {
      state.layout = toggleLayout.checked ? "auto" : "as-designed";
      switchScene();
    });
  }

  var toggleTheme = document.getElementById("toggle-theme");
  var prefersDark = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
  toggleTheme.checked = data.theme === "dark" || (data.theme === "auto" && prefersDark);
  toggleTheme.addEventListener("change", function () {
    document.documentElement.setAttribute("data-theme", toggleTheme.checked ? "dark" : "light");
  });

  /* --------------------------------------------------------------- export */

  function serializeScene() {
    var scene = activeScene();
    var defs = canvas.querySelector("defs");
    var w = scene.getAttribute("data-width");
    var h = scene.getAttribute("data-height");
    var css = toggleTheme.checked ? data.css.dark : data.css.light;
    var clone = scene.cloneNode(true);
    clone.removeAttribute("class");
    clone.querySelectorAll(".is-selected, .is-related, .is-match, .is-field-target").forEach(function (el) {
      el.classList.remove("is-selected", "is-related", "is-match", "is-field-target");
    });
    return (
      '<svg xmlns="http://www.w3.org/2000/svg" width="' + w + '" height="' + h + '" viewBox="0 0 ' + w + " " + h + '">' +
      "<style>" + css + "</style>" +
      defs.outerHTML +
      '<rect class="bg" x="0" y="0" width="' + w + '" height="' + h + '"/>' +
      clone.innerHTML +
      "</svg>"
    );
  }

  function download(blob, filename) {
    var url = URL.createObjectURL(blob);
    var link = document.createElement("a");
    link.href = url;
    link.download = filename;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    setTimeout(function () { URL.revokeObjectURL(url); }, 1000);
  }

  function baseName() {
    return (data.title || "catalog").replace(/[^A-Za-z0-9._-]+/g, "_");
  }

  document.getElementById("export-svg").addEventListener("click", function () {
    download(new Blob([serializeScene()], { type: "image/svg+xml" }), baseName() + ".svg");
  });

  document.getElementById("export-png").addEventListener("click", function () {
    var scene = activeScene();
    var w = parseFloat(scene.getAttribute("data-width"));
    var h = parseFloat(scene.getAttribute("data-height"));
    var scale = 2;
    var image = new Image();
    // A data: URL keeps the canvas untainted, which a blob: URL does not
    // reliably do when the page itself was opened from file://.
    image.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(serializeScene());
    image.onload = function () {
      var target = document.createElement("canvas");
      target.width = Math.round(w * scale);
      target.height = Math.round(h * scale);
      var context = target.getContext("2d");
      context.scale(scale, scale);
      context.drawImage(image, 0, 0);
      if (target.toBlob) {
        target.toBlob(function (blob) { if (blob) download(blob, baseName() + ".png"); });
      }
    };
  });

  /* ----------------------------------------------------------- initialise */

  window.addEventListener("resize", function () {
    sizeCanvas();
    fit();
  });

  sizeCanvas();
  buildMinimap();
  fit();
  applyHash();
})();
