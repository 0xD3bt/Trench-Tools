(function initAutoSellFeature(global) {
  function createAutoSellFeature(config) {
    const {
      elements,
      getNamedValue,
      setNamedValue,
      isNamedChecked,
      formatSliderValue,
      syncSettingsCapabilities,
      syncActivePresetFromInputs,
      validateFieldByName,
      documentNode,
    } = config;

    const {
      devAutoSellButton,
      devAutoSellPanel,
      autoSellEnabledInput,
      autoSellToggleState,
      autoSellTriggerValue,
      autoSellDelaySlider,
      autoSellDelayControl,
      autoSellPercentSlider,
      autoSellDelayValue,
      autoSellBlockControl,
      autoSellBlockValue,
      autoSellPercentValue,
      autoSellSettings,
      autoSellTriggerModeButtons,
      autoSellBlockOffsetButtons,
    } = elements;

    let eventsBound = false;

    function normalizeTriggerMode(value) {
      const mode = String(value || "").trim().toLowerCase();
      if (mode === "submit-delay" || mode === "block-offset") {
        return mode;
      }
      if (mode === "confirmation") return "block-offset";
      return "block-offset";
    }

    function getTriggerMode() {
      return normalizeTriggerMode(getNamedValue("automaticDevSellTriggerMode"));
    }

    function getDelayMs() {
      const numeric = Number(getNamedValue("automaticDevSellDelayMs") || "0");
      if (!Number.isFinite(numeric)) return 0;
      return Math.max(0, Math.min(1500, numeric));
    }

    function getBlockOffset() {
      const numeric = Number(getNamedValue("automaticDevSellBlockOffset") || "0");
      if (!Number.isFinite(numeric)) return 0;
      return Math.max(0, Math.min(22, Math.round(numeric)));
    }

    function getTriggerLabel(mode = getTriggerMode()) {
      if (mode === "submit-delay") return "On Submit + Delay";
      return "On Confirmed Block";
    }

    function getTriggerDescription(mode = getTriggerMode()) {
      if (mode === "submit-delay") {
        return `Sell ${getDelayMs()}ms after submit is observed without waiting for confirmation.`;
      }
      if (mode === "block-offset") {
        return `Send the sell transaction on confirmed block + ${getBlockOffset()} from launch confirmation.`;
      }
      return `Send the sell transaction on confirmed block + ${getBlockOffset()} from launch confirmation.`;
    }

    function getBlockOffsetDescription(offset = getBlockOffset()) {
      return `Send the sell transaction on confirmed block + ${offset} from launch confirmation.`;
    }

    function getSummaryText(formValues) {
      const percent = `${formValues.automaticDevSellPercent || "0"}%`;
      const mode = normalizeTriggerMode(formValues.automaticDevSellTriggerMode);
      if (mode === "submit-delay") {
        return `${percent} at submit + ${Number(formValues.automaticDevSellDelayMs || 0)}ms`;
      }
      if (mode === "block-offset") {
        return `${percent} on confirmed + ${Number(formValues.automaticDevSellBlockOffset || 0)}`;
      }
      return `${percent} on confirmed + ${Number(formValues.automaticDevSellBlockOffset || 0)}`;
    }

    function togglePanel(forceOpen) {
      if (!devAutoSellPanel) return;
      const shouldOpen = typeof forceOpen === "boolean" ? forceOpen : devAutoSellPanel.hidden;
      devAutoSellPanel.hidden = !shouldOpen;
    }

    function syncUI() {
      const enabled = isNamedChecked("automaticDevSellEnabled");
      const rawPercent = Number(getNamedValue("automaticDevSellPercent") || "100");
      const percent = String(Math.max(1, Math.min(100, Number.isFinite(rawPercent) ? rawPercent : 100)));
      const triggerMode = getTriggerMode();
      const delayMs = String(getDelayMs());
      const blockOffset = String(getBlockOffset());
      if (enabled && getNamedValue("automaticDevSellPercent") !== percent) {
        setNamedValue("automaticDevSellPercent", percent);
      }
      if (getNamedValue("automaticDevSellTriggerMode") !== triggerMode) {
        setNamedValue("automaticDevSellTriggerMode", triggerMode);
      }
      if (getNamedValue("automaticDevSellDelayMs") !== delayMs) {
        setNamedValue("automaticDevSellDelayMs", delayMs);
      }
      if (getNamedValue("automaticDevSellBlockOffset") !== blockOffset) {
        setNamedValue("automaticDevSellBlockOffset", blockOffset);
      }

      if (devAutoSellButton) devAutoSellButton.classList.toggle("active", enabled);
      if (autoSellToggleState) autoSellToggleState.textContent = enabled ? "ON" : "OFF";
      if (autoSellEnabledInput) autoSellEnabledInput.checked = enabled;
      if (autoSellSettings) autoSellSettings.hidden = !enabled;
      if (autoSellTriggerValue) autoSellTriggerValue.textContent = getTriggerLabel(triggerMode);
      autoSellTriggerModeButtons.forEach((button) => {
        const buttonMode = button.getAttribute("data-auto-sell-trigger-mode") || "block-offset";
        button.classList.toggle("active", buttonMode === triggerMode);
        button.disabled = !enabled;
        button.title = getTriggerDescription(buttonMode);
      });
      if (autoSellDelaySlider) {
        autoSellDelaySlider.value = delayMs;
        autoSellDelaySlider.disabled = !enabled || triggerMode !== "submit-delay";
      }
      if (autoSellDelayControl) autoSellDelayControl.hidden = !enabled || triggerMode !== "submit-delay";
      if (autoSellBlockControl) autoSellBlockControl.hidden = !enabled || triggerMode !== "block-offset";
      autoSellBlockOffsetButtons.forEach((button) => {
        const offsetValue = Number(button.getAttribute("data-auto-sell-block-offset") || "0");
        button.classList.toggle("active", String(offsetValue) === blockOffset);
        button.disabled = !enabled || triggerMode !== "block-offset";
        button.title = getBlockOffsetDescription(offsetValue);
      });
      if (autoSellPercentSlider) {
        autoSellPercentSlider.value = percent;
        autoSellPercentSlider.disabled = !enabled;
      }
      if (autoSellDelayValue) autoSellDelayValue.textContent = formatSliderValue(delayMs, "ms", 0);
      if (autoSellBlockValue) autoSellBlockValue.textContent = blockOffset;
      if (autoSellPercentValue) autoSellPercentValue.textContent = formatSliderValue(percent, "%", 0);
      syncSettingsCapabilities();
    }

    function bindEvents() {
      if (eventsBound) return;
      eventsBound = true;

      if (devAutoSellButton) {
        devAutoSellButton.addEventListener("click", (event) => {
          event.stopPropagation();
          togglePanel();
        });
      }
      if (autoSellEnabledInput) {
        autoSellEnabledInput.addEventListener("change", () => {
          if (autoSellEnabledInput.checked) {
            const currentPercent = Number(getNamedValue("automaticDevSellPercent") || "0");
            if (!Number.isFinite(currentPercent) || currentPercent <= 0) {
              setNamedValue("automaticDevSellPercent", "100");
            }
          }
          syncUI();
          syncActivePresetFromInputs();
          validateFieldByName("automaticDevSellPercent");
          validateFieldByName("automaticDevSellDelayMs");
          validateFieldByName("automaticDevSellBlockOffset");
        });
      }
      autoSellTriggerModeButtons.forEach((button) => {
        button.addEventListener("click", () => {
          setNamedValue("automaticDevSellTriggerMode", button.getAttribute("data-auto-sell-trigger-mode") || "block-offset");
          syncUI();
          syncActivePresetFromInputs();
          validateFieldByName("automaticDevSellDelayMs");
          validateFieldByName("automaticDevSellBlockOffset");
        });
      });
      if (autoSellDelaySlider) {
        autoSellDelaySlider.addEventListener("input", () => {
          setNamedValue("automaticDevSellDelayMs", autoSellDelaySlider.value);
          syncUI();
          syncActivePresetFromInputs();
          validateFieldByName("automaticDevSellDelayMs");
        });
      }
      autoSellBlockOffsetButtons.forEach((button) => {
        button.addEventListener("click", () => {
          setNamedValue("automaticDevSellBlockOffset", button.getAttribute("data-auto-sell-block-offset") || "0");
          syncUI();
          syncActivePresetFromInputs();
          validateFieldByName("automaticDevSellBlockOffset");
        });
      });
      if (autoSellPercentSlider) {
        autoSellPercentSlider.addEventListener("input", () => {
          setNamedValue("automaticDevSellPercent", autoSellPercentSlider.value);
          syncUI();
          syncActivePresetFromInputs();
          validateFieldByName("automaticDevSellPercent");
        });
      }
      documentNode.addEventListener("click", (event) => {
        if (!devAutoSellPanel || devAutoSellPanel.hidden) return;
        if (devAutoSellPanel.contains(event.target) || (devAutoSellButton && devAutoSellButton.contains(event.target))) return;
        togglePanel(false);
      });
    }

    return {
      bindEvents,
      normalizeTriggerMode,
      getTriggerMode,
      getDelayMs,
      getBlockOffset,
      getTriggerLabel,
      getTriggerDescription,
      getSummaryText,
      syncUI,
      togglePanel,
    };
  }

  global.AutoSellFeature = {
    create: createAutoSellFeature,
  };
})(window);
