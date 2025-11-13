import socket
from struct import pack
import importlib.util
import numpy as np
import zlib

def _area_interpolate(im, scale):
    new_h = im.shape[0] // scale
    new_w = im.shape[1] // scale
    clip_h = new_h * scale
    clip_w = new_w * scale
    buf = np.zeros((new_h, new_w, im.shape[2]), dtype=np.float32)
    for i in range(scale):
        for j in range(scale):
            buf += im[i:clip_h:scale, j:clip_w:scale]
    return (buf / (scale * scale)).astype(im.dtype)

def _parse_dtype(dtype):
    if dtype == np.float64: return 6
    if dtype == np.float32: return 5
    if dtype == np.float16: return 7
    if dtype == np.uint16:  return 2
    if dtype == np.uint8:   return 0
    if dtype == np.int32:   return 4
    if dtype == np.int16:   return 3
    if dtype == np.int8:    return 1
    raise Exception('dtype not supported: ' + str(dtype))

def _to_hwc(arr: np.ndarray) -> np.ndarray:
    """(B,C,H,W)/(C,H,W)/(H,W) -> (H,W,C)"""
    a = arr

    # remove leading 1-dims
    while a.ndim > 3:
        a = a[0]

    if a.ndim == 2:
        return a[:, :, None]  # (H,W,1)
    if a.ndim != 3:
        raise Exception(f'invalid shape: {arr.shape}')

    H, W, C = a.shape
    if a.shape[0] in (1, 3, 4) and a.shape[2] not in (1, 3, 4):
        a = np.transpose(a, (1, 2, 0))

    if a.ndim != 3:
        raise Exception(f'could not normalize shape to HWC: {arr.shape} -> {a.shape}')
    return a

class EdolView:
    def __init__(self, host: str, port: int):
        self.host = host
        self.port = port

    def send_image(self, name: str, image: np.ndarray, float_to_half: bool, do_compression: bool = False, downscale_factor: int = 1):
        # torch -> numpy 변환
        if not isinstance(image, np.ndarray):
            torch_spec = importlib.util.find_spec('torch')
            if torch_spec is not None:
                import torch  # noqa
                if isinstance(image, torch.Tensor):
                    if hasattr(image, 'detach'):
                        image = image.detach()
                    if hasattr(image, 'cpu'):
                        image = image.cpu()
                    image = image.numpy()
        if not isinstance(image, np.ndarray):
            raise Exception('image should be np.ndarray')

        image = _to_hwc(image)
        initial_shape = tuple(image.shape)

        if downscale_factor != 1:
            image = _area_interpolate(image, downscale_factor)

        if image.shape[2] > 4:
            raise Exception('image channel must be <= 4, got shape: ' + str(initial_shape))

        if do_compression and (image.dtype in (np.float32, np.float64)) and float_to_half:
            image = image.astype(np.float16)

        cv2_spec = importlib.util.find_spec('cv2')
        compression = 'raw'
        buf_bytes = None

        if do_compression:
            if np.issubdtype(image.dtype, np.integer) and cv2_spec is not None:
                import cv2
                img_enc = image
                if image.shape[2] == 3:
                    img_enc = image[:, :, ::-1]
                ok, buf = cv2.imencode('.png', img_enc)
                if not ok:
                    raise Exception('cv2.imencode(.png) failed')
                buf_bytes = buf.tobytes()
                compression = 'png'
            else:
                if not image.flags['C_CONTIGUOUS']:
                    image = image.copy()
                buf_bytes = zlib.compress(image.tobytes())
                compression = 'zlib'

        if buf_bytes is None:
            if not image.flags['C_CONTIGUOUS']:
                image = image.copy()
            buf_bytes = image.tobytes()
            compression = 'raw'

        nbytes_uncompressed = image.nbytes
        H, W, C = image.shape
        dtype_code = _parse_dtype(image.dtype)

        compression_bytes = compression.encode('utf-8')
        extra_bytes = b''.join([
            pack('!Q', nbytes_uncompressed),     # u64
            pack('!III', H, W, C),               # 3×u32
            pack('!I', dtype_code),              # u32
            compression_bytes                    # utf-8
        ])

        name_bytes = name.encode('utf-8')

        name_len = len(name_bytes)
        extra_len = len(extra_bytes)
        buf_len = len(buf_bytes)

        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.connect((self.host, self.port))
            print(f'sending image {name} to {self.host}:{self.port}, payload={buf_len/1024:.1f} KB, comp={compression}')

            s.sendall(pack('!Q', name_len))
            s.sendall(pack('!Q', extra_len))
            s.sendall(pack('!Q', buf_len))
            s.sendall(name_bytes)
            s.sendall(extra_bytes)
            s.sendall(buf_bytes)
            s.close()

def send(address: str, name: str, image: np.ndarray, float_to_half: bool = False, do_compression: bool = False, downscale_factor: int = 1):
    host, port_str = address.split(":")
    port = int(port_str)
    edolview = EdolView(host, port)
    edolview.send_image(name, image, float_to_half, do_compression, downscale_factor)