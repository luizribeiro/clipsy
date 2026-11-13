if exists("g:loaded_clipsy")
    finish
endif
let g:loaded_clipsy = 1

function! s:enabled() abort
  if exists('$CLIPSY_ENABLED')
    return $CLIPSY_ENABLED ==# '1'
  endif
  return !empty($SSH_CONNECTION) || !empty($CLIPSY_SERVER)
endfunction

if !s:enabled()
  finish
endif

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
