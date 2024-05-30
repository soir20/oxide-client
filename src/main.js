const { invoke } = window.__TAURI__.tauri
const { message } = window.__TAURI__.dialog
const { BaseDirectory, createDir, readTextFile, writeTextFile } = window.__TAURI__.fs
const { appDataDir, resolveResource } = window.__TAURI__.path

const SAVED_SERVERS_LIST_ID = 'saved-servers'
const SAVED_SERVERS_PATH = 'saved-servers.json'
const USER_SETTINGS_PATH = 'settings.json'
const settings = {}

const I18N_CLASS_NAME = 'i18n'
const I18N_KEY_ATTR = 'data-i18n-key'
const I18N_GLOBAL_CONFIG_PATH = 'i18n.json'
const LANGUAGES = {}
const DEFAULT_LANGUAGE = 'en-US'

function debounce(callback, wait) {
  let timeoutId = null
  return (...args) => {
    window.clearTimeout(timeoutId)
    timeoutId = window.setTimeout(() => callback(...args), wait)
  }
}

async function loadSettings() {
  try {
    Object.assign(settings, JSON.parse(await readTextFile(USER_SETTINGS_PATH, { dir: BaseDirectory.AppData })))
  } catch (err) {
    console.error('Unable to read settings:', err)
  }
}

async function writeTextToAppData(fileName, text) {
  try {
    await createDir(await appDataDir(), {recursive: true})
    await writeTextFile(fileName, text, {dir: BaseDirectory.AppData})
  } catch (err) {
    console.error('Unable to write saved servers:', err)
    message(
      `${getI18nValueForKey(settings.language, 'saved-servers-write-failed')}\n${err}`,
      {
        okLabel: getI18nValueForKey(settings.language, 'ok'),
        type: 'error'
      }
    )
  }
}

function prettyPrintJson(jsonObject) {
  return JSON.stringify(jsonObject, null, 2)
}

async function saveSettings() {
  await writeTextToAppData(USER_SETTINGS_PATH, prettyPrintJson(settings))
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
async function loadLanguageConfig(path) {
  Object.assign(LANGUAGES, JSON.parse(await readTextFile(path)))
}

async function initLanguageSelector(languageSelector) {
  if (!(settings.language in LANGUAGES)) {
    settings.language = DEFAULT_LANGUAGE
  }

  for (const [langId, langValues] of Object.entries(LANGUAGES)) {
    const option = document.createElement('option')
    option.textContent = langValues.name
    option.value = langId

    if (langId === settings.language) {
      option.selected = true
    }

    languageSelector.append(option)
  }

  languageSelector.addEventListener('change', async (event) => {
    settings.language = event.target.value
    loadI18n(settings.language, document)
    await saveSettings()
  })
}

function getI18nValueForKey(langId, key) {
  if (!LANGUAGES[langId][key]) {
    throw new Error(`Unknown i18n key ${key} for language ${langId}`)
  }

  return LANGUAGES[langId][key]
}

function loadI18n(langId, parent) {
  if (!(langId in LANGUAGES)) {
    throw new Error(`Unknown language ${langId}`)
  }

  for (const elm of parent.querySelectorAll('.i18n')) {
    const key = elm.getAttribute(I18N_KEY_ATTR)
    if (!key) {
      throw new Error(`Element ${elm.localName} (id: ${elm.id}) is missing i18n key`)
    }

    elm.innerHTML = getI18nValueForKey(settings.language, key)
  }
}

// Saved server read/write
let savedServers = []

function serverIndex(savedServersElm, currentElm) {
  return Array.from(savedServersElm.children).indexOf(currentElm)
}

function buildTextInput(labelI18nKey, propertyName, savedServer, savedServersElm, serverElm) {
  const label = document.createElement('label')
  const labelText = document.createElement('span')
  labelText.setAttribute(I18N_KEY_ATTR, labelI18nKey)
  labelText.classList.add(I18N_CLASS_NAME)
  label.append(labelText)

  loadI18n(settings.language, label)

  const input = document.createElement('input')
  input.type = 'text'
  input.value = savedServer[propertyName] || ''
  label.append(input)

  input.addEventListener('input', debounce(
    async (event) => {
      savedServers[serverIndex(savedServersElm, serverElm)][propertyName] = event.target.value
      await saveServerList()
    },
    500
  ))

  return label
}

function buildSavedServerElement(savedServersElm, savedServer, isEditing) {
  const serverElm = document.createElement('li')
  serverElm.draggable = true

  // Nickname container
  const nicknameContainer = document.createElement('div')
  nicknameContainer.classList.add('saved-server-nickname-container')
  serverElm.append(nicknameContainer)

  const nickname = document.createElement('input')
  nickname.classList.add('saved-server-nickname')
  nickname.name = "saved-server-nickname"
  nickname.disabled = true
  nickname.type = 'text'
  nickname.value = savedServer.nickname
  nickname.addEventListener('input', debounce(
    async (event) => {
      savedServers[serverIndex(savedServersElm, serverElm)].nickname = event.target.value
      await saveServerList()
    },
    500
  ))
  nicknameContainer.append(nickname)

  const buttonContainer = document.createElement('div')
  buttonContainer.classList.add('saved-servers-main-button-container')
  nicknameContainer.append(buttonContainer)

  const editButton = document.createElement('button')
  editButton.classList.add(I18N_CLASS_NAME)
  editButton.setAttribute(I18N_KEY_ATTR, 'saved-servers-edit')
  buttonContainer.append(editButton)

  const playButton = document.createElement('button')
  playButton.classList.add(I18N_CLASS_NAME)
  playButton.setAttribute(I18N_KEY_ATTR, 'saved-servers-play')
  buttonContainer.append(playButton)

  // Edit container
  const editContainer = document.createElement('div')
  editContainer.classList.add('edit-container')

  const endpointContainer = document.createElement('div')
  editContainer.append(endpointContainer)

  endpointContainer.append(
    buildTextInput(
      'saved-servers-udp-endpoint-label',
      'udp-endpoint',
      savedServer,
      savedServersElm,
      serverElm
    )
  )
  endpointContainer.append(
    buildTextInput(
      'saved-servers-https-endpoint-label',
      'https-endpoint',
      savedServer,
      savedServersElm,
      serverElm
    )
  )

  const editButtonContainer = document.createElement('div')
  editContainer.append(editButtonContainer)

  const removeButton = document.createElement('button')
  removeButton.classList.add(I18N_CLASS_NAME)
  removeButton.setAttribute(I18N_KEY_ATTR, 'saved-servers-remove')

  removeButton.addEventListener('click', async (_) => {
    savedServers.splice(serverIndex(savedServersElm, serverElm), 1)
    serverElm.remove()
    await saveServerList()
  })

  editButtonContainer.append(removeButton)

  const toggleEdit = () => {
    editButton.classList.toggle('edit-button-open')
    editContainer.classList.toggle('edit-container-open')
    nickname.disabled = !nickname.disabled
    nickname.classList.toggle('saved-server-nickname-edit')
  }

  if (isEditing) {
    toggleEdit()
  }
  serverElm.append(editContainer)

  editButton.addEventListener('click', (_) => toggleEdit())

  loadI18n(settings.language, serverElm)

  return serverElm
}

async function loadSavedServers() {
  try {
    savedServers = JSON.parse(await readTextFile(SAVED_SERVERS_PATH, { dir: BaseDirectory.AppData }))
  } catch (err) {
    savedServers = []
    console.error('Unable to read saved servers:', err)
  }

  const savedServersElm = document.getElementById(SAVED_SERVERS_LIST_ID)
  for (const savedServer of savedServers) {
    savedServersElm.append(buildSavedServerElement(savedServersElm, savedServer, false))
  }
}

async function saveServerList() {
  await writeTextToAppData(SAVED_SERVERS_PATH, prettyPrintJson(savedServers))
}

async function addSavedServer(nickname) {
  const savedServer = { nickname }
  savedServers.unshift(savedServer)
  const savedServersElm = document.getElementById(SAVED_SERVERS_LIST_ID)
  savedServersElm.prepend(buildSavedServerElement(savedServersElm, savedServer, true))

  await saveServerList()
}

async function reorderSavedServers(previousIndex, newIndex) {
  let server = savedServers[previousIndex]
  savedServers.splice(previousIndex, 1)
  savedServers.splice(newIndex, 0, server)
  await saveServerList()
}

function initDraggableList(parentList, callback) {
  parentList.classList.add('draggable-list')
  let currentElement = null
  let previousIndex = null

  parentList.addEventListener('dragstart', (event) => {
    currentElement = event.target
    previousIndex = serverIndex(parentList, currentElement)
    setTimeout(() => {
      event.target.classList.add('dragged')
    }, 0)
  })

  parentList.addEventListener('dragend', async (event) => {
    let previousIndexCopy = previousIndex
    let newIndex = serverIndex(parentList, event.target)
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

async function main() {
  await loadSettings()
  await loadLanguageConfig(await resolveResource(I18N_GLOBAL_CONFIG_PATH))
  await initLanguageSelector(document.getElementById('language-selector'))
  initTabs()
  initDraggableList(document.getElementById(SAVED_SERVERS_LIST_ID), reorderSavedServers)
  loadI18n(settings.language, document)
  await loadSavedServers()

  document.getElementById('create-saved-server-btn').addEventListener('click', (e) => {
    e.preventDefault()
    addSavedServer(getI18nValueForKey(settings.language, 'saved-servers-default-name'))
  })
}

await main()
