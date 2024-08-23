# `freebsd-rustdate`

## Intro

This is `freebsd-rustdate` , a reimplementation of `freebsd-update` . It’s primarily written because of [how slow `freebsd-update` is](https://rustdate.over-yonder.net/faq.html#slow), and is [written in Rust](https://rustdate.over-yonder.net/faq.html#rust).

In usage, it’s expected to be **similar, but not identical** to `freebsd-update` . There are probably a number of minor edge-case differences I don’t even know about, but there are a number of larger ones that are intentional too.

> [!CAUTION]
> **This is currently an early, experimental version.** It seems to work OK, and hasn’t blown up any of the systems I’ve used it on. But upgrading your OS is a serious task, and a dangerous one to mess up. Don’t consider this a first-line production tool yet.

## Download

See the [downloads page](https://rustdate.over-yonder.net/download.html) for download links and quickstart instructions.

## Basic usage

As with `freebsd-update` , the basic usage of `freebsd-rustdate` mostly falls into the “fetch” (update current release to new patches) and “upgrade” (update current release to new release) paths.

```sh
$ freebsd-rustdate fetch
$ freebsd-rustdate install
```

```sh
$ freebsd-rustdate upgrade -r 13.8-RELEASE
... run `freebsd-rustdate resolve-merges` if you have conflicts

$ freebsd-rustdate install
... reboot new kernel

$ freebsd-rustdate install
... rebuild packages with new world

$ freebsd-rustdate install
```

The basic configuration is read out of the same `freebsd-update.conf` as `freebsd-update` uses. The subset that `freebsd-rustdate` can use, it does.

## Details

* [Usage](https://rustdate.over-yonder.net/usage.html) describes the details of running `freebsd-rustdate` and the available commands and options. And gives some details about the differences from `freebsd-update` .
    
* [Missing](https://rustdate.over-yonder.net/missing.html) covers some intentionally missing things.
    
* [Speed](https://rustdate.over-yonder.net/speed.html) gives some numbers for the speedup.
    
* [FAQ](https://rustdate.over-yonder.net/faq.html) has meandering musings about stuff you don’t care about.
    
* [Download](https://rustdate.over-yonder.net/download.html) when you’re ready to play with it.
    

## Are you sure it works?

The server this site is running on was upgraded with it. Are you able to load this page?

Now, is it as well tested and widely used as `freebsd-update` ? **Absolutely not**. It’s had basic development and a little use by one guy. Use at your own risk, and it’s probably a somewhat elevated risk. I _strongly recommend_ you don’t try it for the first time on a system you’d have trouble recovering if it broke. Or the second time.

## Bugs

Certainly not. Any current behavior is definitely a feature.

## Thanks

To Colin for writing the original `freebsd-update` and making at all work. Nothing said here should be taken as a slight to him or the work he did to get this up and running. For what I’ve done standing on his shoulders, I can only apologize.

## Contact

[Matthew Fuller](mailto:fullermd@over-yonder.net)  
[https://www.over-yonder.net/~fullermd/](https://www.over-yonder.net/~fullermd/)
