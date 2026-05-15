(function initLaunchDeckImageCropDomain(global) {
  function createImageCropDomain(config = {}) {
    const {
      resolveDisplayUrl = async (value) => value,
      onError = () => {},
    } = config;

    let activeOverlay = null;

    function close() {
      activeOverlay?.remove();
      activeOverlay = null;
    }

    function escapeHTML(value) {
      const div = document.createElement("div");
      div.textContent = String(value || "");
      return div.innerHTML;
    }

    function iconCropSvg() {
      return '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 2v14h16"></path><path d="M18 22V8H2"></path></svg>';
    }

    function dataUrlMimeType(dataUrl) {
      const match = String(dataUrl || "").match(/^data:([^;,]+)[;,]/i);
      return match ? match[1].toLowerCase() : "";
    }

    function imageExtensionForMime(mimeType) {
      const normalized = String(mimeType || "").trim().toLowerCase();
      if (normalized === "image/jpeg" || normalized === "image/jpg") return "jpg";
      if (normalized === "image/webp") return "webp";
      if (normalized === "image/gif") return "gif";
      return "png";
    }

    async function fileFromDataUrl(dataUrl, baseName = "cropped-image") {
      const response = await fetch(dataUrl);
      const blob = await response.blob();
      const mimeType = blob.type || dataUrlMimeType(dataUrl) || "image/png";
      const safeName = String(baseName || "cropped-image")
        .replace(/[^A-Za-z0-9_-]+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 56) || "cropped-image";
      return new File([blob], `${safeName}.${imageExtensionForMime(mimeType)}`, {
        type: mimeType,
      });
    }

    function renderedImageRect(image) {
      const rect = image.getBoundingClientRect();
      const naturalWidth = Number(image.naturalWidth || 0);
      const naturalHeight = Number(image.naturalHeight || 0);
      if (!naturalWidth || !naturalHeight || !rect.width || !rect.height) return rect;
      const naturalRatio = naturalWidth / naturalHeight;
      const boxRatio = rect.width / rect.height;
      if (naturalRatio > boxRatio) {
        const height = rect.width / naturalRatio;
        return {
          left: rect.left,
          top: rect.top + ((rect.height - height) / 2),
          width: rect.width,
          height,
        };
      }
      const width = rect.height * naturalRatio;
      return {
        left: rect.left + ((rect.width - width) / 2),
        top: rect.top,
        width,
        height: rect.height,
      };
    }

    function renderSelection(selection, image, start, current) {
      if (!selection || !image || !start || !current) return;
      const imageRect = renderedImageRect(image);
      const canvasRect = selection.parentElement?.getBoundingClientRect();
      if (!canvasRect) return;
      const x = Math.min(start.x, current.x);
      const y = Math.min(start.y, current.y);
      const width = Math.abs(current.x - start.x);
      const height = Math.abs(current.y - start.y);
      selection.hidden = width < 4 || height < 4;
      Object.assign(selection.style, {
        left: `${imageRect.left - canvasRect.left + x}px`,
        top: `${imageRect.top - canvasRect.top + y}px`,
        width: `${width}px`,
        height: `${height}px`,
      });
    }

    function applySnipLayout(image) {
      if (!image) return;
      Object.assign(image.style, {
        position: "",
        left: "",
        top: "",
        width: "",
        height: "",
        maxWidth: "100%",
        maxHeight: "100%",
        transform: "",
        cursor: "crosshair",
      });
    }

    function initPanCrop(canvas, image, panBox, state) {
      if (!canvas || !image || !panBox || !image.naturalWidth || !image.naturalHeight) return;
      const canvasRect = canvas.getBoundingClientRect();
      const boxSize = Math.max(120, Math.min(300, Math.floor(Math.min(canvasRect.width, canvasRect.height) * 0.6)));
      panBox.style.width = `${boxSize}px`;
      panBox.style.height = `${boxSize}px`;
      const baseScale = Math.max(boxSize / image.naturalWidth, boxSize / image.naturalHeight);
      state.scale = Math.max(state.scale || 0, baseScale);
      state.panX = Number.isFinite(state.panX) ? state.panX : 0;
      state.panY = Number.isFinite(state.panY) ? state.panY : 0;
      Object.assign(image.style, {
        position: "absolute",
        left: "50%",
        top: "50%",
        width: `${image.naturalWidth}px`,
        height: `${image.naturalHeight}px`,
        maxWidth: "none",
        maxHeight: "none",
        cursor: "grab",
      });
      clampPanCrop(image, panBox, state);
      applyPanTransform(image, state);
    }

    function clampPanCrop(image, panBox, state) {
      if (!image || !panBox || !image.naturalWidth || !image.naturalHeight) return;
      const boxSize = panBox.getBoundingClientRect().width || 300;
      const minScale = Math.max(boxSize / image.naturalWidth, boxSize / image.naturalHeight);
      state.scale = Math.max(minScale, state.scale || minScale);
      const scaledWidth = image.naturalWidth * state.scale;
      const scaledHeight = image.naturalHeight * state.scale;
      const maxX = Math.max(0, (scaledWidth - boxSize) / 2);
      const maxY = Math.max(0, (scaledHeight - boxSize) / 2);
      state.panX = Math.max(-maxX, Math.min(maxX, state.panX || 0));
      state.panY = Math.max(-maxY, Math.min(maxY, state.panY || 0));
    }

    function applyPanTransform(image, state) {
      if (!image) return;
      image.style.transform = `translate(calc(-50% + ${state.panX}px), calc(-50% + ${state.panY}px)) scale(${state.scale})`;
    }

    function cropSnipToDataUrl(image, start, current, options = {}) {
      const rect = renderedImageRect(image);
      const width = Math.abs(current.x - start.x);
      const height = Math.abs(current.y - start.y);
      if (width < 4 || height < 4 || !image.naturalWidth || !image.naturalHeight) return "";
      const sx = Math.max(0, Math.round(Math.min(start.x, current.x) * (image.naturalWidth / rect.width)));
      const sy = Math.max(0, Math.round(Math.min(start.y, current.y) * (image.naturalHeight / rect.height)));
      const sw = Math.min(image.naturalWidth - sx, Math.round(width * (image.naturalWidth / rect.width)));
      const sh = Math.min(image.naturalHeight - sy, Math.round(height * (image.naturalHeight / rect.height)));
      return cropRectToDataUrl(image, sx, sy, sw, sh, options);
    }

    function cropPanToDataUrl(image, panBox, state, options = {}) {
      if (!image || !panBox || !image.naturalWidth || !image.naturalHeight) return "";
      const boxSize = panBox.getBoundingClientRect().width || 300;
      const scaledWidth = image.naturalWidth * state.scale;
      const scaledHeight = image.naturalHeight * state.scale;
      const sx = Math.max(0, Math.round(((scaledWidth - boxSize) / 2 - state.panX) / state.scale));
      const sy = Math.max(0, Math.round(((scaledHeight - boxSize) / 2 - state.panY) / state.scale));
      const side = Math.min(
        image.naturalWidth - sx,
        image.naturalHeight - sy,
        Math.round(boxSize / state.scale),
      );
      return cropRectToDataUrl(image, sx, sy, side, side, options);
    }

    function cropRectToDataUrl(image, sx, sy, sw, sh, options = {}) {
      if (sw < 4 || sh < 4) return "";
      const maxOutputSize = Math.max(128, Math.min(2048, Number(options.maxOutputSize || 1024)));
      const scale = Math.min(1, maxOutputSize / Math.max(sw, sh));
      const canvas = document.createElement("canvas");
      canvas.width = Math.max(1, Math.round(sw * scale));
      canvas.height = Math.max(1, Math.round(sh * scale));
      const context = canvas.getContext("2d");
      context.drawImage(image, sx, sy, sw, sh, 0, 0, canvas.width, canvas.height);
      return canvas.toDataURL(options.mimeType || "image/png", 0.92);
    }

    async function saveResult(source, dataUrl, options, closeOnSave = true) {
      if (!dataUrl) return;
      const file = await fileFromDataUrl(dataUrl, `${source.name || "image"}-cropped`);
      await options.onSave?.({
        dataUrl,
        file,
        source,
        mode: source.mode || "",
      });
      if (closeOnSave) close();
    }

    async function open(options = {}) {
      const sourceUrl = String(options.src || "").trim();
      if (!sourceUrl) throw new Error("Image source missing.");
      close();
      const displayUrl = await resolveDisplayUrl(sourceUrl);
      const overlay = document.createElement("div");
      overlay.className = "image-crop-overlay";
      overlay.innerHTML = `
        <div class="image-crop-modal is-snip-mode" role="dialog" aria-modal="true" aria-label="${escapeHTML(options.title || "Crop Image")}">
          <div class="image-crop-header">
            <h3>${escapeHTML(options.title || "Crop Image")}</h3>
            <button type="button" class="image-crop-mode-toggle" data-image-crop-mode-toggle title="Switch crop mode">${iconCropSvg()}<span>Pan mode</span></button>
          </div>
          <p class="image-crop-help" data-image-crop-help>Click and drag to snip an area. Release to crop instantly.</p>
          <div class="image-crop-canvas is-snip" data-image-crop-canvas>
            <img class="image-crop-image" src="${escapeHTML(displayUrl)}" alt="${escapeHTML(options.alt || options.name || "Image")}" draggable="false" crossorigin="anonymous">
            <div class="image-crop-pan-overlay" hidden><div class="image-crop-pan-box"></div></div>
            <div class="image-crop-selection" hidden></div>
          </div>
          <div class="image-crop-actions">
            ${(Array.isArray(options.saveActions) && options.saveActions.length)
              ? options.saveActions.map((action) => `<button type="button" class="image-crop-save ${action.kind === "replace" ? "is-danger" : ""}" data-image-crop-action="${escapeHTML(action.kind)}">${escapeHTML(action.label)}</button>`).join("")
              : '<button type="button" class="image-crop-save" data-image-crop-action="save">Crop & Save</button>'}
          </div>
          <div class="image-crop-error" data-image-crop-error></div>
        </div>
      `;
      document.body.appendChild(overlay);
      activeOverlay = overlay;
      bindOverlay(overlay, options);
      return undefined;
    }

    function bindOverlay(overlay, options) {
      const modal = overlay.querySelector(".image-crop-modal");
      const canvas = overlay.querySelector("[data-image-crop-canvas]");
      const image = overlay.querySelector(".image-crop-image");
      const selection = overlay.querySelector(".image-crop-selection");
      const panOverlay = overlay.querySelector(".image-crop-pan-overlay");
      const panBox = overlay.querySelector(".image-crop-pan-box");
      const help = overlay.querySelector("[data-image-crop-help]");
      const modeToggle = overlay.querySelector("[data-image-crop-mode-toggle]");
      const errorNode = overlay.querySelector("[data-image-crop-error]");
      const state = {
        mode: "snip",
        start: null,
        current: null,
        snipDataUrl: "",
        draggingPan: false,
        panStartClientX: 0,
        panStartClientY: 0,
        panStartX: 0,
        panStartY: 0,
        panX: 0,
        panY: 0,
        scale: 1,
      };

      const setError = (message = "") => {
        if (errorNode) errorNode.textContent = message;
        if (message) onError(message);
      };

      const setMode = (mode) => {
        state.mode = mode;
        state.start = null;
        state.current = null;
        state.snipDataUrl = "";
        state.draggingPan = false;
        if (selection) selection.hidden = true;
        modal?.classList.toggle("is-pan-mode", mode === "pan");
        modal?.classList.toggle("is-snip-mode", mode === "snip");
        canvas?.classList.toggle("is-pan", mode === "pan");
        canvas?.classList.toggle("is-snip", mode === "snip");
        if (panOverlay) panOverlay.hidden = mode !== "pan";
        if (help) {
          help.textContent = mode === "pan"
            ? "Drag the image to position it inside the 1:1 box. Scroll to zoom."
            : "Click and drag to snip an area. Release to crop instantly.";
        }
        if (modeToggle) {
          modeToggle.querySelector("span").textContent = mode === "pan" ? "Snip mode" : "Pan mode";
          modeToggle.title = mode === "pan" ? "Switch to click-and-drag snip mode" : "Switch to fixed 1:1 pan mode";
        }
        if (mode === "pan") initPanCrop(canvas, image, panBox, state);
        else applySnipLayout(image);
      };

      const pointFromEvent = (event) => {
        if (!image) return null;
        const rect = renderedImageRect(image);
        return {
          x: Math.max(0, Math.min(rect.width, event.clientX - rect.left)),
          y: Math.max(0, Math.min(rect.height, event.clientY - rect.top)),
        };
      };

      async function runSave(kind = "save") {
        try {
          setError("");
          const dataUrl = state.mode === "pan"
            ? cropPanToDataUrl(image, panBox, state, options)
            : state.snipDataUrl || (state.start && state.current ? cropSnipToDataUrl(image, state.start, state.current, options) : "");
          if (!dataUrl) return;
          await saveResult({ ...options, mode: state.mode, saveKind: kind }, dataUrl, {
            ...options,
            onSave: (payload) => options.onSave?.({ ...payload, kind }),
          }, true);
        } catch (error) {
          setError(error.message || "Crop failed.");
        }
      }

      overlay.addEventListener("click", (event) => {
        if (event.target === overlay) close();
      });
      document.addEventListener("keydown", function onKeydown(event) {
        if (activeOverlay !== overlay) {
          document.removeEventListener("keydown", onKeydown);
          return;
        }
        if (event.key === "Escape") {
          event.preventDefault();
          close();
          document.removeEventListener("keydown", onKeydown);
        } else if (event.key === "Enter" && state.mode === "pan") {
          event.preventDefault();
          runSave("save");
        }
      });
      modeToggle?.addEventListener("click", () => setMode(state.mode === "pan" ? "snip" : "pan"));
      canvas?.addEventListener("pointerdown", (event) => {
        event.preventDefault();
        canvas.setPointerCapture?.(event.pointerId);
        if (state.mode === "pan") {
          state.draggingPan = true;
          state.panStartClientX = event.clientX;
          state.panStartClientY = event.clientY;
          state.panStartX = state.panX;
          state.panStartY = state.panY;
          canvas.classList.add("is-dragging");
          return;
        }
        state.start = pointFromEvent(event);
        state.current = state.start ? { ...state.start } : null;
        renderSelection(selection, image, state.start, state.current);
      });
      canvas?.addEventListener("pointermove", (event) => {
        if (state.mode === "pan" && state.draggingPan) {
          state.panX = state.panStartX + (event.clientX - state.panStartClientX);
          state.panY = state.panStartY + (event.clientY - state.panStartClientY);
          clampPanCrop(image, panBox, state);
          applyPanTransform(image, state);
          return;
        }
        if (!state.start) return;
        state.current = pointFromEvent(event);
        renderSelection(selection, image, state.start, state.current);
      });
      canvas?.addEventListener("pointerup", async () => {
        if (state.mode === "pan") {
          state.draggingPan = false;
          canvas.classList.remove("is-dragging");
          return;
        }
        if (!image || !state.start || !state.current) return;
        const width = Math.abs((state.current.x || 0) - state.start.x);
        const height = Math.abs((state.current.y || 0) - state.start.y);
        if (width >= 8 && height >= 8) {
          state.snipDataUrl = cropSnipToDataUrl(image, state.start, state.current, options);
          const defaultActionKind = String(options.defaultActionKind || options.defaultKind || "").trim();
          if (defaultActionKind || !Array.isArray(options.saveActions) || options.saveActions.length <= 1) {
            await runSave(defaultActionKind || "save");
          }
        }
        state.start = null;
        state.current = null;
        if (selection) selection.hidden = true;
      });
      canvas?.addEventListener("pointercancel", () => {
        state.start = null;
        state.current = null;
        state.draggingPan = false;
        canvas.classList.remove("is-dragging");
        if (selection) selection.hidden = true;
      });
      canvas?.addEventListener("wheel", (event) => {
        if (state.mode !== "pan") return;
        event.preventDefault();
        const nextScale = Math.max(0.05, Math.min(8, state.scale * (event.deltaY < 0 ? 1.08 : 0.92)));
        if (Math.abs(nextScale - state.scale) < 0.001) return;
        const rect = canvas.getBoundingClientRect();
        const anchorX = event.clientX - (rect.left + rect.width / 2);
        const anchorY = event.clientY - (rect.top + rect.height / 2);
        const ratio = nextScale / state.scale;
        state.panX = anchorX - (anchorX - state.panX) * ratio;
        state.panY = anchorY - (anchorY - state.panY) * ratio;
        state.scale = nextScale;
        clampPanCrop(image, panBox, state);
        applyPanTransform(image, state);
      }, { passive: false });
      overlay.querySelectorAll("[data-image-crop-action]").forEach((button) => {
        button.addEventListener("click", () => runSave(button.getAttribute("data-image-crop-action") || "save"));
      });
      image?.addEventListener("load", () => {
        if (state.mode === "pan") initPanCrop(canvas, image, panBox, state);
      });
      image?.addEventListener("error", () => setError("Image failed to load."));
      setMode("snip");
      modal?.focus?.();
    }

    return {
      open,
      close,
      fileFromDataUrl,
    };
  }

  global.LaunchDeckImageCropDomain = {
    create: createImageCropDomain,
  };
})(window);
