if exists("g:loaded_clipsy")
    finish
endif
let g:loaded_clipsy = 1

let g:clipboard = {
  \   'name': 'clipsy',
  \   'copy': {
  \      '+': ['clipsy', 'write'],
  \      '*': ['clipsy', 'write'],
  \    },
  \   'paste': {
  \      '+': ['clipsy', 'read'],
  \      '*': ['clipsy', 'read'],
  \   },
  \   'cache_enabled': 1,
  \ }
