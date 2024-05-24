const { invoke } = window.__TAURI__.tauri
const { BaseDirectory, createDir, readTextFile, writeTextFile } = window.__TAURI__.fs
const { appDataDir } = window.__TAURI__.path
import { LANGUAGES } from './i18n.js'

// Tab functionality
const TAB_PREFIX = 'tab-'
const TAB_CONTENT_PREFIX = 'tab-content-'

let activeTabName = 'servers'
function changeTab(tabName) {
  const oldTab = document.querySelector(`#${TAB_PREFIX}${activeTabName}`)
  oldTab.classList.remove(`${TAB_PREFIX}active`)

  const oldContent = document.querySelector(`#${TAB_CONTENT_PREFIX}${activeTabName}`)
  if (!oldContent) {
    throw new Error(`Tab ${activeTabName} is missing content`)
  }
  oldContent.classList.remove(`${TAB_CONTENT_PREFIX}active`)

  const newTab = document.querySelector(`#${TAB_PREFIX}${tabName}`)
  newTab.classList.add(`${TAB_PREFIX}active`)

  const newContent = document.querySelector(`#${TAB_CONTENT_PREFIX}${tabName}`)
  if (!newContent) {
    throw new Error(`Tab ${tabName} is missing content`)
  }
  newContent.classList.add(`${TAB_CONTENT_PREFIX}active`)

  activeTabName = tabName
}

function initTabs() {
  for (const tab of document.querySelectorAll('.tab')) {
    tab.addEventListener('click', (e) => {
      e.preventDefault()
      if (!tab.id.startsWith(TAB_PREFIX)) {
        throw Error(`Tab ID ${tab.id} is missing tab prefix '${TAB_CONTENT_PREFIX}'`)
      }
      changeTab(tab.id.replace(TAB_PREFIX, ''))
    })
  }
}

// Internationalization
function loadI18n(langId) {
  if (!(langId in LANGUAGES)) {
    throw new Error(`Unknown language ${langId}`)
  }

  for (const elm of document.querySelectorAll('.i18n')) {
    const key = elm.getAttribute('data-i18n-key')
    if (!key) {
      throw new Error(`Element ${elm.localName} (id: ${elm.id}) is missing i18n key`)
    }

    if (!LANGUAGES[langId][key]) {
      throw new Error(`Unknown i18n key ${key} for language ${langId}`)
    }

    elm.innerHTML = LANGUAGES[langId][key]
  }
}

// Saved server read/write
const SAVED_SERVERS_FILE = 'saved-servers.json'
let savedServers = []
function buildSavedServerElement(savedServer) {
  const serverElm = document.createElement('li')
  serverElm.textContent = savedServer.nickname
  return serverElm
}

async function loadSavedServers(parent) {
  try {
    savedServers = JSON.parse(await readTextFile(SAVED_SERVERS_FILE, { dir: BaseDirectory.AppData }))
  } catch (err) {
    console.error('Unable to read saved servers', err)
  }

  for (const savedServer of savedServers) {
    parent.append(buildSavedServerElement(savedServer))
  }
}

async function main() {
  initTabs()
  loadI18n('en-US')

  await createDir(await appDataDir(), { recursive: true })
  //await writeTextFile('saved-servers.json', '[ { "hello": "world" } ]', { dir: BaseDirectory.AppData })

  await loadSavedServers(document.getElementById('saved-servers'))
}

await main()
