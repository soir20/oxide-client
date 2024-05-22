const { invoke } = window.__TAURI__.tauri

// Tab functionality
const tabPrefix = "tab-"
const tabContentPrefix = "tab-content-"

let activeTabName = "servers"
for (const tab of document.querySelectorAll(".tab")) {
  tab.addEventListener("click", (e) => {
    e.preventDefault()
    if (!tab.id.startsWith(tabPrefix)) {
      throw Error(`Tab ID ${tab.id} is missing tab prefix "${tabContentPrefix}"`)
    }
    changeTab(tab.id.replace(tabPrefix, ""))
  })
}

function changeTab(tabName) {
  const oldTab = document.querySelector(`#${tabPrefix}${activeTabName}`)
  oldTab.classList.remove(`${tabPrefix}active`)

  const oldContent = document.querySelector(`#${tabContentPrefix}${activeTabName}`)
  if (!oldContent) {
    throw new Error(`Tab ${activeTabName} is missing content`)
  }
  oldContent.classList.remove(`${tabContentPrefix}active`)

  const newTab = document.querySelector(`#${tabPrefix}${tabName}`)
  newTab.classList.add(`${tabPrefix}active`)

  const newContent = document.querySelector(`#${tabContentPrefix}${tabName}`)
  if (!newContent) {
    throw new Error(`Tab ${tabName} is missing content`)
  }
  newContent.classList.add(`${tabContentPrefix}active`)

  activeTabName = tabName
}

// Internationalization
import { LANGUAGES } from './i18n.js'
function loadI18n(langId) {
  if (!(langId in LANGUAGES)) {
    throw new Error(`Unknown language ${langId}`)
  }

  for (const elm of document.querySelectorAll(".i18n")) {
    const key = elm.getAttribute("data-i18n-key")
    if (!key) {
      throw new Error(`Element ${elm.localName} (id: ${elm.id}) is missing i18n key`)
    }

    if (!LANGUAGES[langId][key]) {
      throw new Error(`Unknown i18n key ${key} for language ${langId}`)
    }

    elm.innerHTML = LANGUAGES[langId][key]
  }
}

loadI18n("en_US")
