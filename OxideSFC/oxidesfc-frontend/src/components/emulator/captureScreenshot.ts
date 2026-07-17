import { save } from '@tauri-apps/plugin-dialog';
import { writeFile } from '@tauri-apps/plugin-fs';

export type ScreenshotResult = 'saved' | 'cancelled';

/**
 * Captures the emulator's WebGL <canvas> client-side (there is no backend
 * `take_screenshot` command -- see WebGLRenderer.ts's `preserveDrawingBuffer:
 * true`, which keeps the drawing buffer intact so this capture isn't blank)
 * and writes it to disk via the same dialog+fs plugins used elsewhere in
 * this app (see FileSystemService.ts).
 *
 * Shared by the control deck's screenshot button, the F8 hotkey, and the
 * quick menu. Throws on failure; returns 'cancelled' if the user dismissed
 * the save dialog.
 */
export async function captureScreenshot(
  canvas: HTMLCanvasElement,
  gameTitle?: string
): Promise<ScreenshotResult> {
  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob(resolve, 'image/png');
  });

  if (!blob) {
    throw new Error('canvas.toBlob returned no data');
  }

  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  const safeTitle = (gameTitle || 'screenshot').replace(/[\\/:*?"<>|]/g, '_');
  const defaultPath = `${safeTitle}_${timestamp}.png`;

  const destination = await save({
    title: 'Save Screenshot',
    defaultPath,
    filters: [{ name: 'PNG Image', extensions: ['png'] }],
  });

  if (!destination) {
    return 'cancelled';
  }

  const buffer = new Uint8Array(await blob.arrayBuffer());
  await writeFile(destination, buffer);
  return 'saved';
}
