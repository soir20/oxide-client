const { invoke } = window.__TAURI__.tauri
const { message } = window.__TAURI__.dialog

const SAVED_SERVERS_LIST_ID = 'saved-servers'
const SAVED_SERVER_WRITE_FAILED_I18N_KEY = 'saved-servers-write-failed'
const SETTINGS_WRITE_FAILED_I18N_KEY = 'settings-write-failed'
const I18N_CLASS_NAME = 'i18n'
const I18N_KEY_ATTR = 'data-i18n-key'

function debounce(callback, wait) {
  let timeoutId = null
  return (...args) => {
    window.clearTimeout(timeoutId)
    timeoutId = window.setTimeout(() => callback(...args), wait)
  }
}

async function try_or_show_err_dialog(promise, i18n_key) {
  try {
    return await promise
  } catch (err) {
    console.error('Unable to write saved servers:', err)
    message(
      `${await getI18nValueForKey(i18n_key)}\n${err}`,
      {
        okLabel: await getI18nValueForKey('ok'),
        type: 'error'
      }
    )
  }
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
async function initLanguageSelector(languageSelector) {
  const currentLangId = await invoke('current_language_id')
  for (const [langId, langName] of await invoke('all_language_ids_names')) {
    const option = document.createElement('option')
    option.textContent = langName
    option.value = langId

    if (langId === currentLangId) {
      option.selected = true
    }

    languageSelector.append(option)
  }

  languageSelector.addEventListener('change', async (event) => {
    await try_or_show_err_dialog(invoke('set_language', event.target.value), SETTINGS_WRITE_FAILED_I18N_KEY)
    await loadI18n(document)
  })
}

async function getI18nValueForKey(key) {
  return await invoke('i18n_value_for_key', { key })
}

async function loadI18n(parent) {
  for (const elm of parent.querySelectorAll('.i18n')) {
    const key = elm.getAttribute(I18N_KEY_ATTR)
    if (!key) {
      throw new Error(`Element ${elm.localName} (id: ${elm.id}) is missing i18n key`)
    }

    elm.innerHTML = await getI18nValueForKey(key)
  }
}

// Saved server read/write
function serverIndex(savedServersElm, currentElm) {
  return Array.from(savedServersElm.children).indexOf(currentElm)
}

async function buildTextInput(labelI18nKey, callback, initValue, savedServersElm, serverElm) {
  const label = document.createElement('label')
  const labelText = document.createElement('span')
  labelText.setAttribute(I18N_KEY_ATTR, labelI18nKey)
  labelText.classList.add(I18N_CLASS_NAME)
  label.append(labelText)

  await loadI18n(label)

  const input = document.createElement('input')
  input.type = 'text'
  input.value = initValue
  label.append(input)

  input.addEventListener('input', debounce(
    async (event) => {
      await callback(serverIndex(savedServersElm, serverElm), event.target.value)
    },
    500
  ))

  return label
}

async function buildSavedServerElement(savedServersElm, savedServer, isEditing) {
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
      await try_or_show_err_dialog(invoke('set_saved_server_nickname', { index: serverIndex(savedServersElm, serverElm), nickname: event.target.value }), SAVED_SERVER_WRITE_FAILED_I18N_KEY)
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
    await buildTextInput(
      'saved-servers-udp-endpoint-label',
      async (index, udpEndpoint) => await try_or_show_err_dialog(invoke('set_saved_server_udp_endpoint', { index, udpEndpoint }), SAVED_SERVER_WRITE_FAILED_I18N_KEY),
      savedServer.udp_endpoint,
      savedServersElm,
      serverElm
    )
  )
  endpointContainer.append(
    await buildTextInput(
      'saved-servers-https-endpoint-label',
      async (index, httpsEndpoint) => await try_or_show_err_dialog(invoke('set_saved_server_https_endpoint', { index, httpsEndpoint }), SAVED_SERVER_WRITE_FAILED_I18N_KEY),
      savedServer.https_endpoint,
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
    await try_or_show_err_dialog(invoke('remove_saved_server', { index: serverIndex(savedServersElm, serverElm) }), SAVED_SERVER_WRITE_FAILED_I18N_KEY)
    serverElm.remove()
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

  await loadI18n(serverElm)

  return serverElm
}

async function loadSavedServers() {
  let savedServers = await try_or_show_err_dialog(invoke('load_saved_servers'), SAVED_SERVER_WRITE_FAILED_I18N_KEY)
  const savedServersElm = document.getElementById(SAVED_SERVERS_LIST_ID)
  for (const savedServer of savedServers) {
    savedServersElm.append(await buildSavedServerElement(savedServersElm, savedServer, false))
  }
}

async function addSavedServer(nickname) {
  const savedServer = { nickname, udp_endpoint: '', https_endpoint: '' }
  const savedServersElm = document.getElementById(SAVED_SERVERS_LIST_ID)

  await try_or_show_err_dialog(invoke('add_saved_server', { savedServer }), SAVED_SERVER_WRITE_FAILED_I18N_KEY)
  savedServersElm.prepend(await buildSavedServerElement(savedServersElm, savedServer, true))
}

async function reorderSavedServers(oldIndex, newIndex) {
  await try_or_show_err_dialog(invoke('reorder_saved_servers', { oldIndex, newIndex }), SAVED_SERVER_WRITE_FAILED_I18N_KEY)
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
  await initLanguageSelector(document.getElementById('language-selector'))
  initTabs()
  initDraggableList(document.getElementById(SAVED_SERVERS_LIST_ID), reorderSavedServers)
  await loadI18n(document)
  await loadSavedServers()

  document.getElementById('create-saved-server-btn').addEventListener('click', async (e) => {
    e.preventDefault()
    await addSavedServer(await getI18nValueForKey('saved-servers-default-name'))
  })
}

await main()
