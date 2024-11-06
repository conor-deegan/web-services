// Imports
import { json, urlencoded } from 'body-parser';
import cors from 'cors';
import express, { Application, NextFunction, Request, Response } from 'express';

// Create application
const app: Application = express();

// Application config
app.use(cors());

app.use(json({ limit: '50mb' }));

app.use(urlencoded({ extended: true, limit: '50mb' }));

const cacheStore: {
    [key: string]: string;
} = {};

// Routes
app.post('/set', (req: Request, res: Response) => {
    const { key, value } = req.body;
    console.log(`Setting key: ${key} and value: ${value}`);
    const data = {
        key,
        value
    };
    cacheStore[key] = value;
    res.status(201).json({
        data
    });
});

app.get('/get/:key', (req: Request, res: Response) => {
    const { key } = req.params;
    console.log(`Getting key: ${key}`);
    const value = cacheStore[key];
    if (value) {
        const parsedValue = JSON.parse(value);
        res.status(200).json(parsedValue);
    } else {
        res.status(404).json({
            data: 'Not Found'
        });
    }
});

// Health check
app.get('/healthz', (_req, res) => {
    res.status(200).json({
        data: 'ok'
    });
});

// Catch all
app.use('*', (req, res) => {
    res.status(404).json({
        data: 'Not Found'
    });
});

const PORT = 8005;

app.listen(PORT, () => {
    console.log(`Cache is running on port ${PORT}`);
});