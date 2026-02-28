// Lightweight message hiding for performance
(function() {
  function optimizeDOMTree() {
    const messages = document.querySelectorAll('article, [class*="group"]');
    const viewportTop = window.scrollY;
    const viewportBottom = viewportTop + window.innerHeight;
    const buffer = 1500;
    
    messages.forEach(msg => {
      const rect = msg.getBoundingClientRect();
      const absoluteTop = rect.top + viewportTop;
      const absoluteBottom = rect.bottom + viewportTop;
      
      const isInRange = absoluteBottom > (viewportTop - buffer) && 
                        absoluteTop < (viewportBottom + buffer);
      
      if (!isInRange && msg.style.display !== 'none') {
        msg.style.display = 'none';
      } else if (isInRange && msg.style.display === 'none') {
        msg.style.display = '';
      }
    });
  }

  let scrollTimer;
  window.addEventListener('scroll', () => {
    clearTimeout(scrollTimer);
    scrollTimer = setTimeout(optimizeDOMTree, 150);
  }, { passive: true });

  window.addEventListener('load', () => {
    setTimeout(optimizeDOMTree, 1000);
  });
})();
