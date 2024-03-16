# 📋 clipsy

Sync your clipboard across hosts.

## Usage

Start the server on your laptop:

```bash
clipsy serve
```

Then setup your SSH to forward port 52697 by adding this to your
`~/.ssh/config`:

```
Host *
  RemoteForward 52697 127.0.0.1:52697
```

Then, from the remote host you can write to your clipboard with:

```
clipsy write foobar
```

Alternatively, you can also stream to your clipboard:

```
cat somefile.txt | clipsy write
```
