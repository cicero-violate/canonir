// Injected page script: intercepts SSE and emits captures
(function() {
  // ─────────────────────────────────────────────
  // Button Registry (Single Source of Truth)
  // ─────────────────────────────────────────────

  const BUTTON_REGISTRY = {
    judge: {
      id: "__mmsb_judge_btn",
      label: "Judge",
      color: "#333",
      location: "code_block",
        messageType: "USER_COMMAND",
      extractCode: true,
      handler: (codeText) => ({ action: "Judge", text: codeText })
    },

    insert: {
      id: "__mmsb_insert_btn",
      label: "Insert",
      color: "#0a5",
      location: "code_block",
        messageType: "OUTBOUND_SUBMIT",
      extractCode: true,
      handler: (codeText) => ({ text: codeText, mode: "buffer" })
    },

    run: {
      id: "__mmsb_submit_btn",
      label: "Submit",
      color: "#0b5",
      location: "toolbar",
        messageType: "USER_COMMAND",
      handler: () => {
        if (window.__promptInjectionQueue?.length > 0) {
          window.__pendingPromptInjection = window.__promptInjectionQueue.join('\n\n');
          window.__promptInjectionQueue = [];
        }
        return { action: "Submit" };
      },
      clickElement: 'button[data-testid="send-button"]'
    },

    system: {
      id: "__mmsb_system_btn",
      label: "System",
      color: "#f80",
      location: "toolbar",
        messageType: "USER_PROMPT",
      handler: () => ({ action: "LoadPrompts" })
    }
  };

  // ─────────────────────────────────────────────
  // Generic Button Factory
  // ─────────────────────────────────────────────

  function createButton(config) {
    const btn = document.createElement("button");
    btn.textContent = config.label;
    btn.dataset.mmsbButton = "1";
    btn.dataset.mmsbButtonType = config.label.toLowerCase();

    // Styling based on location
    const isToolbar = config.location === "toolbar";
    btn.style.marginLeft = isToolbar ? "8px" : "6px";
    btn.style.padding = isToolbar ? "4px 8px" : "2px 6px";
    btn.style.fontSize = isToolbar ? "14px" : "12px";
    btn.style.border = "1px solid #888";
    btn.style.borderRadius = isToolbar ? "6px" : "4px";
    btn.style.background = config.color;
    btn.style.color = "#fff";
    btn.style.cursor = "pointer";

    btn.onclick = () => handleButtonClick(config, btn);
    return btn;
  }

  function handleButtonClick(config, button) {
    let payload;

    // Extract code if required
    if (config.extractCode) {
      const codeBlock = findCodeBlock(button);
      const text = codeBlock?.innerText || codeBlock?.textContent || "";
      payload = config.handler(text);
    } else {
      payload = config.handler();
    }

    // Click target element if specified (e.g., send button)
    if (config.clickElement) {
      setTimeout(() => {
        const target = document.querySelector(config.clickElement);
        if (target && !target.disabled) {
          target.click();
        } else {
          // Fallback: press Enter in editor
          submitViaEnter();
        }
      }, 100);
    }

     // Mark Judge action to suppress backend calls
     if (config.messageType === "USER_COMMAND" && payload.action === "Judge") {
      window.__isJudgeAction = true;
      setTimeout(() => { window.__isJudgeAction = false; }, 1000);
    }

    // Dispatch event
    dispatchButtonEvent(config.messageType, payload);
  }

  // ─────────────────────────────────────────────
  // Submit via synthetic Enter key
  // ─────────────────────────────────────────────

  function submitViaEnter() {
    const editor = document.querySelector('div[contenteditable="true"]');
    if (!editor) return;

    editor.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        code: "Enter",
        which: 13,
        keyCode: 13,
        bubbles: true,
        cancelable: true
      })
    );
  }

  function dispatchButtonEvent(messageType, payload) {
     if (messageType === "USER_COMMAND") {
      document.dispatchEvent(new CustomEvent("MMSB_EXECUTE_CODE", { detail: payload }));
    } else {
      window.postMessage({ type: messageType, payload }, "*");
    }
  }

  function findCodeBlock(button) {
    let node = button.parentElement;
    while (node && !node.querySelector("code")) {
      node = node.parentElement;
    }
    return node?.querySelector("code");
  }

  // ─────────────────────────────────────────────
  // Unified Button Injection
  // ─────────────────────────────────────────────

  function injectButtons() {
    injectToolbarButtons();
    injectCodeBlockButtons();
  }

  function injectToolbarButtons() {
    const container = findToolbarContainer();
    if (!container) return;

    Object.values(BUTTON_REGISTRY)
      .filter(cfg => cfg.location === "toolbar")
      .forEach(cfg => {
        // Toolbar buttons remain globally unique
        if (document.getElementById(cfg.id)) return;
        
        const btn = createButton(cfg);
        btn.id = cfg.id; // Only toolbar buttons get IDs
        container.appendChild(btn);
      });
  }

  function injectCodeBlockButtons() {
    const copyButtons = document.querySelectorAll('button[aria-label="Copy"]');
    // console.log('[MMSB] Found', copyButtons.length, 'copy buttons');

    copyButtons.forEach(copyBtn => {
      const bar = copyBtn.parentElement;
      if (!bar) {
        // console.log('[MMSB] No parent bar for copy button');
        return;
      }
      if (bar.querySelector('[data-mmsb-button]')) {
        // console.log('[MMSB] Buttons already injected in this bar');
        return;
      }

      Object.values(BUTTON_REGISTRY)
        .filter(cfg => cfg.location === "code_block")
        .forEach(cfg => {
          const btn = createButton(cfg);
          // console.log('[MMSB] Injecting button:', cfg.label);
          bar.appendChild(btn);
        });
    });
  }

  const __prevFetch = window.fetch;
  if (window.__ChatGPTMessengerBridgeInstalled) return;
  window.__ChatGPTMessengerBridgeInstalled = true;

  // ─────────────────────────────────────────────
  // WebSocket hook (Calpico transport)
  // ─────────────────────────────────────────────

  const __OrigWebSocket = window.WebSocket;

  window.WebSocket = function(url, protocols) {
    const ws = protocols
      ? new __OrigWebSocket(url, protocols)
      : new __OrigWebSocket(url);

    try {
  // WS opened

      ws.addEventListener("message", (ev) => {
        let data = "";

        if (typeof ev.data === "string") {
          data = ev.data;
        } else if (ev.data instanceof ArrayBuffer) {
          data = new TextDecoder().decode(ev.data);
        }

        if (data) {

          window.postMessage(
            {
               type: "INBOUND_MESSAGE",
              payload: data
            },
            "*"
          );
        }
      });
    } catch {}

    return ws;
  };

  window.WebSocket.prototype = __OrigWebSocket.prototype;

  const TARGETS = [
    // Legacy SSE
    { origin: "https://chatgpt.com", path: "/backend-api/f/conversation" },
    { origin: "https://chat.openai.com", path: "/backend-api/f/conversation" },

    // Calpico streaming fetch
    { origin: "https://chatgpt.com", path: "/backend-api/calpico" },
    { origin: "https://chat.openai.com", path: "/backend-api/calpico" }
  ];

  function matchesTarget(input) {
    try {
      const url = new URL(input, location.href);
      for (const t of TARGETS) {
        if (url.origin === t.origin && url.pathname.startsWith(t.path)) return true;
      }
    } catch {}
    return false;
  }

  // NOTE: browser is transport only. no parsing, no filtering.

    // Capture HTTP chunks of chatgpt solo
  const originalFetch = __prevFetch;
   // fetch wrapper installed

  window.fetch = async function(input, init) {
        // fetch called

        // Silence ChatGPT telemetry beacons (Datadog)
        if (typeof input === "string" &&
            input.includes("browser-intake-datadoghq.com")) {
          return new Response(null, { status: 204 });
        }

        const url = matchesTarget(typeof input === 'string' ? input : input?.url);
        // target matched

        const response = await originalFetch(input, init);
        if (!url) return response;

        if (!response.body) {
            return response;
        }

        const [toPage, toCapture] = response.body.tee();


      // HOW MESSAGES ARE BEING RECIEVED BY 
       (async () => {
           const reader = toCapture.getReader();
           const decoder = new TextDecoder();
           let buffer = '';

           try {
               while (true) {
                   const { value, done } = await reader.read();
                   if (done) break;

                    const chunk = decoder.decode(value, { stream: true });
                     // raw chunk

                     // Dual framing support:
                     //  - newline framed (legacy SSE)
                     //  - whole JSON object chunk (calpico)

                     if (chunk.includes('\n')) {
                       buffer += chunk;
                       const lines = buffer.split('\n');
                       buffer = lines.pop();

                        for (const line of lines) {
                           // captured framed line
                          window.postMessage(
                            {
                              type: "INBOUND_MESSAGE",
                              payload: line
                            },
                            "*"
                          );
                        }
                     } else {
                        // captured JSON chunk
                        window.postMessage(
                          {
                            type: "INBOUND_MESSAGE",
                            payload: chunk
                          },
                          "*"
                        );
                     }
               }
           } catch (err) {
               // console.error("[MessengerBridge] Stream error", err);
           }
       })();

       return new Response(toPage, {
           status: response.status,
           statusText: response.statusText,
           headers: response.headers
       });
   };

  // Receive prompt injection requests
  // window.addEventListener("message", (event) => {
  // window.addEventListener("message", (event) => {
  //     if (event.source !== window) return;
  //     if (event.data?.type === "OUTBOUND_SUBMIT") {
  // 	  // Forward structured execution result to page consumers
  // 	  window.postMessage(
  // 	      {
  // 		  type: "OUTBOUND_SUBMIT",
  // 		  payload: event.data.payload
  // 	      },
  // 	      "*"
  // 	  );
  //     }
  // }, true);
  
  // Receive prompt injection requests

//   window.addEventListener("message", (event) => {
//  if (event.source !== window) return;
//    if (event.data?.type === "OUTBOUND_SUBMIT") {
//    const { text, mode } = event.data.payload || {};
//    if (typeof text === "string") {
//      window.__promptInjectionMode = mode || "auto"; // auto | buffer
//      console.log( "[MessengerBridge] Pending prompt injected:", text.length, "mode=", window.__promptInjectionMode );

//      if (mode === "buffer") {
//        // Buffer mode: accumulate in queue, don't submit
//        window.__promptInjectionQueue = window.__promptInjectionQueue || [];
//        if (text) {
//          window.__promptInjectionQueue.push(text);
//        }
//        // Set placeholder if not already present
//        const editor = document.querySelector('div[contenteditable="true"]');
//        if (editor && editor.textContent !== "<PROMPT>") {
//          editor.textContent = "<PROMPT>";
//          editor.dispatchEvent(new Event('input', { bubbles: true }));
//        }
//      } else {

//        // Auto mode can mean either:
//        // 1. New text to inject and submit immediately
//        // 2. Empty text = trigger to submit buffered queue
//        if (text) {
//          window.__pendingPromptInjection = text;
//          const editor = document.querySelector('div[contenteditable="true"]');
//          if (editor) {
//            editor.textContent = "<PROMPT>";
//            editor.dispatchEvent(new Event('input', { bubbles: true }));
//          }
//        }
       
//        // Always auto-submit when mode=auto (whether text is new or from buffer)
//        setTimeout(() => {
//          // Finalize queue into pending before submit
//          if (window.__promptInjectionQueue?.length > 0) {
//            window.__pendingPromptInjection = window.__promptInjectionQueue.join('\n\n');
//            window.__promptInjectionQueue = [];
//          }
         
//          const sendBtn = document.querySelector('button[data-testid="send-button"]');
//          if (sendBtn && !sendBtn.disabled) {
//            sendBtn.click();
//          }
//        }, 100);
//        }
//      }
//    }
// });

// window.addEventListener("message", (event) => {
//   if (event.source !== window) return;
//   if (event.data?.type !== "OUTBOUND_SUBMIT") return;

//   const payload = event.data.payload || {};

//   // ✅ PATCH RESULTS PATH
//   if (payload.reports || payload.failures) {
//     console.log("[Inject] Patch results:", payload);
//     return; // <-- CRITICAL
//   }

//   // ✅ PROMPT INJECTION PATH
//   const { text, mode } = payload;
//   if (typeof text !== "string") return;

//   window.__promptInjectionMode = mode || "auto";

//   if (mode === "buffer") {
//     window.__promptInjectionQueue ||= [];
//     if (text) window.__promptInjectionQueue.push(text);

//     const editor = document.querySelector('div[contenteditable="true"]');
//     if (editor && editor.textContent !== "<PROMPT>") {
//       editor.textContent = "<PROMPT>";
//       editor.dispatchEvent(new Event("input", { bubbles: true }));
//     }
//     return;
//   }

//   if (text) {
//     window.__pendingPromptInjection = text;
//     const editor = document.querySelector('div[contenteditable="true"]');
//     if (editor) {
//       editor.textContent = "<PROMPT>";
//       editor.dispatchEvent(new Event("input", { bubbles: true }));
//     }
//   }

//   setTimeout(() => {
//     if (window.__promptInjectionQueue?.length > 0) {
//       window.__pendingPromptInjection =
//         window.__promptInjectionQueue.join("\n\n");
//       window.__promptInjectionQueue = [];
//     }

//     const sendBtn =
//       document.querySelector('button[data-testid="send-button"]');
//     if (sendBtn && !sendBtn.disabled) {
//       sendBtn.click();
//     }
//   }, 100);
// });

  window.addEventListener("message", (event) => {
    if (event.source !== window) return;
    if (event.data?.type !== "OUTBOUND_SUBMIT") return;

    const payload = event.data.payload || {};

    // ─────────────────────────────────────────────
    // PATCH RESULT PATH (Rust → UI)
    // ─────────────────────────────────────────────
    if (payload.reports || payload.failures) {
      console.log("[Inject] Patch results:", payload);

      window.postMessage({
	type: "OUTBOUND_SUBMIT",
	payload: {
	  text: JSON.stringify(payload, null, 2),
	  mode: "auto"
	}
      }, "*");

      return;
    }

    // ─────────────────────────────────────────────
    // PROMPT INJECTION PATH (Rust → ChatGPT input)
    // ─────────────────────────────────────────────
    const { text, mode } = payload;

    if (typeof text !== "string") return;

    window.__promptInjectionMode = mode || "auto";

    // BUFFER MODE (queue only)
    if (mode === "buffer") {
      window.__promptInjectionQueue ||= [];

      if (text) {
        window.__promptInjectionQueue.push(text);
      }

      const editor = document.querySelector(
        'div[contenteditable="true"]'
      );

      if (editor && editor.textContent !== "<PROMPT>") {
        editor.textContent = "<PROMPT>";
        editor.dispatchEvent(new Event("input", { bubbles: true }));
      }

      return;
    }

    // AUTO MODE
    if (text) {
      window.__pendingPromptInjection = text;

      const editor = document.querySelector(
        'div[contenteditable="true"]'
      );

      if (editor) {
        editor.textContent = "<PROMPT>";
        editor.dispatchEvent(new Event("input", { bubbles: true }));
      }
    }

    // Always submit
    setTimeout(() => {
      if (window.__promptInjectionQueue?.length > 0) {
        window.__pendingPromptInjection =
          window.__promptInjectionQueue.join("\n\n");

        window.__promptInjectionQueue = [];
      }

      const sendBtn =
        document.querySelector('button[data-testid="send-button"]');

      if (sendBtn && !sendBtn.disabled) {
        sendBtn.click();
      } else {
        submitViaEnter();
      }
    }, 100);
  });






// window.addEventListener("message", (event) => {
//   if (event.source !== window) return;

//   if (event.data?.type === "OUTBOUND_SUBMIT") {
//     // const { text, mode } = event.data.payload || {};

//     // const payload = event.data.payload || {};

//     // ✅ HANDLE PATCH REPORTS
//     if (payload.reports || payload.failures) {
//       console.log("[Inject] Patch results:", payload);
//       return;
//     }

//     const summary =
//       payload.summary
// 	? `patch_ok=${payload.summary.patch_ok} patch_fail=${payload.summary.patch_fail}`
// 	: "patch result";

//     if (typeof text === "string") {
//       window.__promptInjectionMode = mode || "auto";

//       if (mode === "buffer") {
//         window.__promptInjectionQueue ||= [];
//         if (text) window.__promptInjectionQueue.push(text);

//         const editor = document.querySelector('div[contenteditable="true"]');
//         if (editor && editor.textContent !== "<PROMPT>") {
//           editor.textContent = "<PROMPT>";
//           editor.dispatchEvent(new Event("input", { bubbles: true }));
//         }
//       } else {
//         if (text) {
//           window.__pendingPromptInjection = text;

//           const editor = document.querySelector('div[contenteditable="true"]');
//           if (editor) {
//             editor.textContent = "<PROMPT>";
//             editor.dispatchEvent(new Event("input", { bubbles: true }));
//           }
//         }

//         // 🔴 THIS MUST EXIST
//         setTimeout(() => {
//           if (window.__promptInjectionQueue?.length > 0) {
//             window.__pendingPromptInjection =
//               window.__promptInjectionQueue.join("\n\n");
//             window.__promptInjectionQueue = [];
//           }

//           const sendBtn =
//             document.querySelector('button[data-testid="send-button"]');

//           if (sendBtn && !sendBtn.disabled) {
//             sendBtn.click();
//           }
//         }, 100);
//       }
//     }
//   }
// });

function findToolbarContainer() {
    const editor = document.querySelector('div[contenteditable="true"]');
    if (!editor) return null;

    let node = editor.parentElement;
    for (let i = 0; i < 5 && node; i++) {
      if (node instanceof HTMLElement) {
        const style = getComputedStyle(node);
        if (style.display.includes("flex") || style.display.includes("grid")) {
          return node;
        }
      }
      node = node.parentElement;
    }
    return editor.parentElement;
  }

  // Observe DOM changes (React-safe)
  const __mmsbObserver = new MutationObserver(injectButtons);

  function startJudgeObserver() {
    // console.log('[MMSB] Starting button observer');
    const root = document.body || document.documentElement;
    if (!(root instanceof Node)) return false;

    __mmsbObserver.observe(root, {
      childList: true,
      subtree: true
    });

    injectButtons();
    return true;
  }

  if (!startJudgeObserver()) {
    document.addEventListener("DOMContentLoaded", startJudgeObserver, {
      once: true
    });
  }
})();
