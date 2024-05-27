const { invoke } = window.__TAURI__.tauri
const { BaseDirectory, createDir, readTextFile, writeTextFile } = window.__TAURI__.fs
const { appDataDir } = window.__TAURI__.path
import { LANGUAGES } from './i18n.js'

let currentLanguage = 'en-US'

async function writeTextToAppData(fileName, text) {
  await createDir(await appDataDir(), { recursive: true })
  await writeTextFile(fileName, text, { dir: BaseDirectory.AppData })
}

function prettyPrintJson(jsonObject) {
  return JSON.stringify(jsonObject, null, 2)
}

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
function loadI18n(langId, parent) {
  if (!(langId in LANGUAGES)) {
    throw new Error(`Unknown language ${langId}`)
  }

  for (const elm of parent.querySelectorAll('.i18n')) {
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
const SAVED_SERVERS_LIST_ID = 'saved-servers'
const SAVED_SERVERS_FILE = 'saved-servers.json'
let savedServers = []
function buildSavedServerElement(savedServer, isEditing) {
  const serverElm = document.createElement('li')
  serverElm.draggable = true

  // Nickname container
  const nicknameContainer = document.createElement('div')
  nicknameContainer.classList.add('saved-server-nickname-container')
  serverElm.append(nicknameContainer)

  const nickname = document.createElement('div')
  nickname.classList.add('saved-server-nickname')
  nickname.textContent = savedServer.nickname
  nicknameContainer.append(nickname)

  const buttonContainer = document.createElement('div')
  nicknameContainer.append(buttonContainer)

  const editButton = document.createElement('button')
  editButton.classList.add('i18n')
  editButton.setAttribute('data-i18n-key', 'saved-servers-edit')
  buttonContainer.append(editButton)

  // Edit container
  const editContainer = document.createElement('div')
  editContainer.classList.add('edit-container')
  if (isEditing) {
    editButton.classList.add('edit-button-open')
    editContainer.classList.add('edit-container-open')
  }
  serverElm.append(editContainer)
  editContainer.textContent = "TEST"

  editButton.addEventListener('click', (_) => {
    editButton.classList.toggle('edit-button-open')
    editContainer.classList.toggle('edit-container-open')
  })

  loadI18n(currentLanguage, serverElm)

  return serverElm
}

async function loadSavedServers() {
  try {
    savedServers = JSON.parse(await readTextFile(SAVED_SERVERS_FILE, { dir: BaseDirectory.AppData }))
  } catch (err) {
    console.error('Unable to read saved servers', err)
  }

  const savedServersElm = document.getElementById(SAVED_SERVERS_LIST_ID)
  for (const savedServer of savedServers) {
    savedServersElm.append(buildSavedServerElement(savedServer, false))
  }
}

async function addSavedServer(nickname, gameServerAddr, authServerAddr) {
  const savedServer = { nickname, gameServerAddr, authServerAddr }
  savedServers.unshift(savedServer)
  const savedServersElm = document.getElementById(SAVED_SERVERS_LIST_ID)
  savedServersElm.prepend(buildSavedServerElement(savedServer, true))

  await writeTextToAppData(SAVED_SERVERS_FILE, prettyPrintJson(savedServers))
}

async function reorderSavedServers(previousIndex, newIndex) {
  let server = savedServers[previousIndex]
  savedServers.splice(previousIndex, 1)
  savedServers.splice(newIndex, 0, server)
  await writeTextToAppData(SAVED_SERVERS_FILE, prettyPrintJson(savedServers))
}

function initDraggableList(parentList, callback) {
  parentList.classList.add('draggable-list')
  let currentElement = null
  let previousIndex = null

  parentList.addEventListener('dragstart', (event) => {
    currentElement = event.target
    previousIndex = Array.from(parentList.children).indexOf(currentElement)
    setTimeout(() => {
      event.target.classList.add('dragged')
    }, 0)
  })

  parentList.addEventListener('dragend', async (event) => {
    let previousIndexCopy = previousIndex
    let newIndex = Array.from(parentList.children).indexOf(event.target)
    setTimeout(() => {
      event.target.classList.remove('dragged')
      currentElement = null
      previousIndex = null
    }, 0)

    if (previousIndexCopy !== null) {
      await callback(previousIndexCopy, newIndex)
    }
  })

  parentList.addEventListener('dragover', (event) => {
    event.preventDefault()
    const nextElement = getNextElement(parentList, event.clientY)
    if (nextElement == null) {
      parentList.appendChild(currentElement)
    } else {
      parentList.insertBefore(currentElement, nextElement)
    }
  })

  function getNextElement(container, y) {
    const draggableElements = [...parentList.children]

    return draggableElements.reduce(
      (closest, child) => {
        const box = child.getBoundingClientRect()
        const offset = y - box.top - box.height / 2
        if (offset < 0 && offset > closest.offset) {
          return {
            offset: offset,
            element: child,
          }
        } else {
          return closest
        }
      },
      {
        offset: Number.NEGATIVE_INFINITY,
      }
    ).element
  }
}

let x = 0
async function main() {
  initTabs()
  initDraggableList(document.getElementById('saved-servers'), reorderSavedServers)
  loadI18n(currentLanguage, document)
  await loadSavedServers()

  document.getElementById('create-saved-server-btn').addEventListener('click', (e) => {
    e.preventDefault()
    addSavedServer(`My Test Server${x++}`, "Hello world", "Test")
  })
}

await main()
