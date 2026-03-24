// Request modifier hook - intercepts outgoing ChatGPT API requests
// and replaces placeholder text with window.__pendingPromptInjection
(function() {
  if (window.__RequestHookPrivateInstalled) return;
  window.__RequestHookPrivateInstalled = true;

  const TARGETS = [
    {origin: "https://chatgpt.com", path: "/backend-api/f/conversation"},
    {origin: "https://chat.openai.com", path: "/backend-api/f/conversation"}
  ];

  console.log('[RequestHookPrivate] Installing request modifier');

  window.__pendingPromptInjection = window.__pendingPromptInjection || null;
  window.__promptInjectionMode = window.__promptInjectionMode || "auto"; // auto | buffer
  window.__promptInjectionQueue = window.__promptInjectionQueue || [];
  window.__promptInjectionQueue = window.__promptInjectionQueue || [];

  function matchesTarget(input) {
    try {
      const abs = new URL(input, location.href);
      for (const target of TARGETS) {
        if (abs.origin === target.origin && abs.pathname.startsWith(target.path)) {
          return true;
        }
      }
    } catch (err) {
      // console.warn('[RequestHook] URL parse failed', err);
    }
    return false;
  }

  const originalFetch = window.fetch;
  window.fetch = async function(input, init) {
    const isTarget = matchesTarget(typeof input === 'string' ? input : input?.url);

    // Suppress backend conversation calls triggered by Judge
    // if (isTarget && window.__isJudgeAction) {
    //    console.warn(
    //      "[RequestHook] Suppressed backend-api/f/conversation during USER_COMMAND action"
    //    );
    //   return new Response(null, { status: 204 });
    // }

    if (isTarget) {
      // console.log('[RequestHook] Intercepted ChatGPT API call');
      console.groupCollapsed('[RequestHook] backend-api/f/conversation');
      // console.log('URL:', typeof input === 'string' ? input : input?.url);
      // console.log('Has pending injection:', Boolean(window.__pendingPromptInjection));
      console.trace('Request stack trace');
      console.groupEnd();
    }

    // Modify outgoing request if we have pending injection
    if (isTarget && init && typeof init.body === 'string' && (window.__pendingPromptInjection || window.__promptInjectionQueue?.length > 0)) {
      try {
        const payload = JSON.parse(init.body);
        // console.log('[RequestHook] Has pending injection:', window.__pendingPromptInjection.substring(0, 50));

        // Replace the placeholder text with actual prompt
        if (Array.isArray(payload?.messages) && payload.messages.length > 0) {
          const lastMessage = payload.messages[payload.messages.length - 1];
          if (lastMessage?.content?.parts) {
            // console.log('[RequestHook] Original parts:', lastMessage.content.parts);
            // Only replace if parts contain the <PROMPT> placeholder
            if (lastMessage.content.parts.some(part => typeof part === 'string' && part.includes('<PROMPT>'))) {
              // Combine queue and pending (filter out nulls)
              const parts = [...window.__promptInjectionQueue];
              if (window.__pendingPromptInjection) {
                parts.push(window.__pendingPromptInjection);
              }
              const combined = parts.join('\n\n');
              lastMessage.content.parts = [combined];
              window.__promptInjectionQueue = [];
              // console.log('[RequestHook] Replaced <PROMPT> with injection');
            } else {
              // console.log('[RequestHook] No <PROMPT> placeholder found, skipping injection');
            }
            // console.log('[RequestHook] Injected prompt:', window.__pendingPromptInjection.substring(0, 100) + '...');
          }
        }
        
        init.body = JSON.stringify(payload);
        if (window.__promptInjectionMode === "auto") {
          window.__pendingPromptInjection = null;
          window.__promptInjectionQueue = [];
        }
      } catch (err) {
        console.warn('[RequestHook] Request modification failed', err);
      }
    }

    return originalFetch(input, init);
  };

  // console.log('[RequestHook] Installed successfully');
})();
