import { isCommand } from "./instructions";

describe("isCommand", () => {
  it("recognises commands to copy", () => {
    for (const line of [
      "powercfg /h off",
      "DISM /Online /Cleanup-Image /StartComponentCleanup",
      "vssadmin resize shadowstorage /for=C: /on=C: /maxsize=10GB",
      "npm install",
      "cargo clean",
      String.raw`robocopy "C:\a" "D:\a" /E /COPY:DAT /DCOPY:T`,
      'Optimize-VHD -Path "<path of the .vhdx>" -Mode Full',
      'select vdisk file="<path of the .vhdx>"',
      "C:\\Windows\\Temp",
      ".venv\\Scripts\\pip install -r requirements.txt",
    ]) {
      expect(isCommand(line), line).toBe(true);
    }
  });

  it("leaves sentences as text", () => {
    for (const line of [
      "Open the folder below in Explorer, select everything and delete it.",
      "Hyper-V (administrator PowerShell):",
      "In Chrome: Settings > Privacy and security > Delete browsing data.",
      "To undo: powercfg /h on",
      "",
    ]) {
      expect(isCommand(line), line).toBe(false);
    }
  });
});
