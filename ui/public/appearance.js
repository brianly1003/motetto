try {
  const appearance = localStorage.getItem('motetto-appearance');
  if (appearance === 'light' || appearance === 'dark') document.documentElement.dataset.theme = appearance;
} catch {}
