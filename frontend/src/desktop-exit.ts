export const BROWSER_EXIT_MESSAGE = "Close this browser tab to leave.";

export async function requestExit(options: {
  inDesktop: boolean;
  closeWindow: () => Promise<void>;
  say: (message: string) => void;
}): Promise<void> {
  if (!options.inDesktop) {
    options.say(BROWSER_EXIT_MESSAGE);
    return;
  }
  await options.closeWindow();
}
