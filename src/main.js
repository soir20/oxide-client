const { invoke } = window.__TAURI__.tauri;

const tabPrefix = "tab-";
const tabContentPrefix = "tab-content-";

let activeTabName = "servers";
for (const tab of document.querySelectorAll(".tab")) {
  tab.addEventListener("click", (e) => {
    e.preventDefault();
    if (!tab.id.startsWith(tabPrefix)) {
      throw Error(`Tab ID ${tab.id} is missing tab prefix "${tabContentPrefix}"`)
    }
    changeTab(tab.id.replace(tabPrefix, ""))
  })
}

function changeTab(tabName) {
  const oldTab = document.querySelector(`#${tabPrefix}${activeTabName}`);
  oldTab.classList.remove(`${tabPrefix}active`);

  const oldContent = document.querySelector(`#${tabContentPrefix}${activeTabName}`);
  if (!oldContent) {
    throw new Error(`Tab ${activeTabName} is missing content`);
  }
  oldContent.classList.remove(`${tabContentPrefix}active`);

  const newTab = document.querySelector(`#${tabPrefix}${tabName}`);
  newTab.classList.add(`${tabPrefix}active`);

  const newContent = document.querySelector(`#${tabContentPrefix}${tabName}`);
  if (!newContent) {
    throw new Error(`Tab ${tabName} is missing content`);
  }
  newContent.classList.add(`${tabContentPrefix}active`);

  activeTabName = tabName;
}

let greetInputEl;
let greetMsgEl;

async function greet() {
  // Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
  greetMsgEl.textContent = await invoke("greet", { name: greetInputEl.value });
}

window.addEventListener("DOMContentLoaded", () => {
  greetInputEl = document.querySelector("#greet-input");
  greetMsgEl = document.querySelector("#greet-msg");
  document.querySelector("#greet-form").addEventListener("submit", (e) => {
    e.preventDefault();
    greet();
  });
});
