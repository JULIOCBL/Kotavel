"""Optional Conan binary remote configured through the consumer environment."""
from urllib.parse import urlsplit


REMOTE_NAME = 'kotavel-binaries'


def configure_remote(conan, env, execute):
    """Register the optional binary cache without changing Conan defaults otherwise."""
    url = env.get('KOTAVEL_CONAN_REMOTE_URL', '').strip()
    if not url:
        return False
    parsed = urlsplit(url)
    if parsed.scheme not in {'https', 'http'} or not parsed.netloc:
        raise RuntimeError('KOTAVEL_CONAN_REMOTE_URL debe ser una URL HTTP(S) válida.')
    username = env.get('KOTAVEL_CONAN_REMOTE_USER', '')
    password = env.get('KOTAVEL_CONAN_REMOTE_PASSWORD', '')
    if bool(username) != bool(password):
        raise RuntimeError('Configura juntos KOTAVEL_CONAN_REMOTE_USER y KOTAVEL_CONAN_REMOTE_PASSWORD.')

    execute([conan, 'remote', 'add', REMOTE_NAME, url, '--index', '0', '--force'], env)
    if username:
        execute([conan, 'remote', 'login', REMOTE_NAME, username, '-p', password], env)
    return True
