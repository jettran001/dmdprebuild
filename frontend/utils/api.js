const getCsrfToken = async () => {
    const response = await fetch('http://localhost:3000/api/csrf-token', {
        method: 'GET',
        headers: {
            'Authorization': `Bearer ${localStorage.getItem('token')}`
        }
    });
    return await response.text();
};

const apiRequest = async (url, method = 'GET', body = null) => {
    const csrfToken = await getCsrfToken();
    const headers = {
        'Authorization': `Bearer ${localStorage.getItem('token')}`,
        'Content-Type': 'application/json',
        'X-CSRF-Token': csrfToken
    };
    const options = { method, headers };
    if (body) options.body = JSON.stringify(body);
    return fetch(url, options).then(res => res.json());
};

export { apiRequest };