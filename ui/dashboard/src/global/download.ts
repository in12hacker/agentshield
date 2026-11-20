// One browser operation for public DER and selected-event JSON downloads.
export function downloadBlob(blob: Blob, filename: string): void {
  const link = document.createElement('a');
  let url: string | null = null;
  let clicked = false;
  try {
    url = URL.createObjectURL(blob);
    link.href = url;
    link.download = filename;
    document.body.append(link);
    link.click();
    clicked = true;
  } finally {
    try { link.remove(); }
    finally {
      if (url !== null) {
        const objectUrl = url;
        const release = () => URL.revokeObjectURL(objectUrl);
        // Let the browser consume the clicked URL before releasing its Blob.
        if (clicked) setTimeout(release, 0);
        else release();
      }
    }
  }
}
